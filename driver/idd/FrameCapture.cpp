#include "FrameCapture.h"
#include "Trace.h"

using Microsoft::WRL::ComPtr;

namespace UsbDisplay
{
    namespace
    {
        // Convert one source pixel to our canonical 32-bpp BGRA layout, i.e. the
        // little-endian uint32 (A<<24)|(R<<16)|(G<<8)|B whose bytes in memory are
        // B,G,R,A -- exactly what a BI_RGB 32bpp BMP and most encoders expect.
        inline uint32_t ToBgra(uint32_t px, DXGI_FORMAT fmt)
        {
            switch (fmt)
            {
            case DXGI_FORMAT_B8G8R8A8_UNORM:
            case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB:
                // Source bytes are already B,G,R,A.
                return px;
            case DXGI_FORMAT_R8G8B8A8_UNORM:
            case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB:
            {
                // Source bytes R,G,B,A -> swap R and B.
                const uint32_t r = px & 0x000000FFu;
                const uint32_t b = (px & 0x00FF0000u) >> 16;
                return (px & 0xFF00FF00u) | (r << 16) | b;
            }
            case DXGI_FORMAT_R10G10B10A2_UNORM:
            {
                // R:0-9 G:10-19 B:20-29 A:30-31. Down-shift each channel to 8 bits.
                const uint32_t r = ((px >> 2) & 0xFFu);
                const uint32_t g = ((px >> 12) & 0xFFu);
                const uint32_t b = ((px >> 22) & 0xFFu);
                return 0xFF000000u | (r << 16) | (g << 8) | b;
            }
            default:
                // Unknown layout: pass through so a frame is still produced; colors
                // may be wrong but the pipeline keeps running and it is visible.
                return px;
            }
        }
    }

    FrameCapturer::FrameCapturer(ComPtr<ID3D11Device> device, ComPtr<ID3D11DeviceContext> context)
        : m_device(std::move(device)), m_context(std::move(context))
    {
        USBLOG_INFO(L"FrameCapture: capturer ready (in-memory only)");
    }

    bool FrameCapturer::EnsureStaging(const D3D11_TEXTURE2D_DESC& srcDesc)
    {
        if (m_staging &&
            m_width == srcDesc.Width &&
            m_height == srcDesc.Height &&
            m_stagingFormat == srcDesc.Format)
        {
            return true;
        }

        m_staging.Reset();

        D3D11_TEXTURE2D_DESC td = {};
        td.Width = srcDesc.Width;
        td.Height = srcDesc.Height;
        td.MipLevels = 1;
        td.ArraySize = 1;
        td.Format = srcDesc.Format;
        td.SampleDesc.Count = 1;
        td.SampleDesc.Quality = 0;
        td.Usage = D3D11_USAGE_STAGING;
        td.BindFlags = 0;
        td.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        td.MiscFlags = 0;

        HRESULT hr = m_device->CreateTexture2D(&td, nullptr, &m_staging);
        if (FAILED(hr))
        {
            USBLOG_ERROR(L"FrameCapture: staging CreateTexture2D failed 0x%08X (%ux%u fmt=%d)",
                         hr, srcDesc.Width, srcDesc.Height, (int)srcDesc.Format);
            return false;
        }

        m_width = srcDesc.Width;
        m_height = srcDesc.Height;
        m_stagingFormat = srcDesc.Format;
        USBLOG_INFO(L"FrameCapture: staging ready %ux%u fmt=%d", m_width, m_height, (int)m_stagingFormat);
        return true;
    }

    bool FrameCapturer::Capture(ID3D11Texture2D* acquiredSurface)
    {
        if (!acquiredSurface || !m_device || !m_context)
        {
            return false;
        }

        D3D11_TEXTURE2D_DESC sd = {};
        acquiredSurface->GetDesc(&sd);
        if (sd.Width == 0 || sd.Height == 0 || sd.Width > 8192 || sd.Height > 8192)
        {
            return false;
        }
        if (sd.SampleDesc.Count > 1)
        {
            // Multisampled surfaces cannot be CopyResource'd into a plain staging
            // texture; IDD composition surfaces are single-sample, so this is a
            // guard rather than an expected path.
            USBLOG_WARN(L"FrameCapture: unexpected multisampled surface (count=%u)", sd.SampleDesc.Count);
            return false;
        }

        if (!EnsureStaging(sd))
        {
            return false;
        }

        // GPU->CPU copy of the whole surface, then a blocking Map. Map on the
        // immediate context flushes and waits for the copy, so the readback is
        // deterministic (no stale-frame race).
        m_context->CopyResource(m_staging.Get(), acquiredSurface);

        D3D11_MAPPED_SUBRESOURCE mapped = {};
        HRESULT hr = m_context->Map(m_staging.Get(), 0, D3D11_MAP_READ, 0, &mapped);
        if (FAILED(hr))
        {
            USBLOG_ERROR(L"FrameCapture: Map staging failed 0x%08X", hr);
            return false;
        }

        m_pixels.resize(static_cast<size_t>(m_width) * m_height);
        const auto* base = static_cast<const uint8_t*>(mapped.pData);
        for (uint32_t y = 0; y < m_height; ++y)
        {
            const auto* srcRow = reinterpret_cast<const uint32_t*>(base + static_cast<size_t>(y) * mapped.RowPitch);
            uint32_t* dstRow = &m_pixels[static_cast<size_t>(y) * m_width];
            for (uint32_t x = 0; x < m_width; ++x)
            {
                dstRow[x] = ToBgra(srcRow[x], m_stagingFormat);
            }
        }

        m_context->Unmap(m_staging.Get(), 0);
        return true;
    }

}
