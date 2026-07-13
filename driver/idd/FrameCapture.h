#pragma once

#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <cstdint>
#include <string>
#include <vector>
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>

namespace UsbDisplay
{
    // Captures the actual composed contents of the USBDisplay virtual monitor.
    //
    // For an Indirect Display Driver the surface handed to us each frame by
    // IddCxSwapChainReleaseAndAcquireBuffer IS the OS-composed image for OUR
    // monitor and nothing else -- it is allocated on the D3D device we passed to
    // IddCxSwapChainSetDevice. So "capturing the virtual monitor" means reading
    // that surface back; it can never contain the whole desktop or any other
    // display. This class does the GPU->CPU readback:
    //
    //   1. Lazily create a CPU-readable STAGING texture matching the surface.
    //   2. CopyResource(staging, acquiredSurface) on the render context.
    //   3. Map the staging texture and normalize the pixels to 32-bpp BGRA,
    //      honoring the mapped RowPitch (which is >= width*4, often padded).
    //
    // The normalized frame is kept in a CPU buffer so it can be written to disk
    // (deterministic validation) today and fed to an encoder later, without any
    // further GPU work.
    class FrameCapturer
    {
    public:
        FrameCapturer(Microsoft::WRL::ComPtr<ID3D11Device> device,
                      Microsoft::WRL::ComPtr<ID3D11DeviceContext> context);

        // Read back one acquired surface into the CPU BGRA buffer. Returns false
        // if the surface is unusable or the format is unsupported (caller keeps
        // running; a transient failure must never be fatal).
        bool Capture(ID3D11Texture2D* acquiredSurface);

        // Write the last captured frame to a 32-bpp top-down BMP. False on I/O
        // failure. Only valid after a successful Capture().
        bool DumpBmp(const std::wstring& path);

        // Directory (created on construction) where frames are dumped, e.g.
        // C:\ProgramData\USBDisplay\capture, or a temp-dir fallback.
        const std::wstring& OutputDir() const { return m_outputDir; }

        uint32_t Width() const { return m_width; }
        uint32_t Height() const { return m_height; }

        // Const view of the last captured frame (BGRA, row-major, top-down,
        // tightly packed width*height). Empty until the first successful Capture.
        // This is the hand-off point for the future hardware encoder.
        const std::vector<uint32_t>& Pixels() const { return m_pixels; }

    private:
        bool EnsureStaging(const D3D11_TEXTURE2D_DESC& srcDesc);
        static std::wstring ChooseOutputDir();

        Microsoft::WRL::ComPtr<ID3D11Device> m_device;
        Microsoft::WRL::ComPtr<ID3D11DeviceContext> m_context;
        Microsoft::WRL::ComPtr<ID3D11Texture2D> m_staging;

        uint32_t m_width = 0;
        uint32_t m_height = 0;
        DXGI_FORMAT m_stagingFormat = DXGI_FORMAT_UNKNOWN;
        std::vector<uint32_t> m_pixels; // BGRA, top-down, width*height
        std::wstring m_outputDir;
    };
}
