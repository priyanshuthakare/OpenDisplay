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
    // Generates a deterministic animated test pattern to prove the presentation
    // pipeline works end-to-end (monitor enumerated -> OS drives swap-chain -> we
    // render) BEFORE any capture/encode is added. Each frame draws:
    //   * scrolling SMPTE-style vertical colour bars
    //   * a moving RGB gradient band
    //   * a bouncing square
    //   * an FPS + frame-counter readout (5x7 bitmap font)
    // The frame is composed in a CPU BGRA buffer, uploaded to a D3D11 texture (to
    // exercise the render device), and periodically written to disk as a BMP so the
    // animation can be visually verified.
    class TestPatternRenderer
    {
    public:
        TestPatternRenderer(Microsoft::WRL::ComPtr<ID3D11Device> device,
                            Microsoft::WRL::ComPtr<ID3D11DeviceContext> context,
                            uint32_t width, uint32_t height);

        // Compose one frame of the pattern and upload it to the GPU texture.
        void Render(uint64_t frameIndex, double elapsedSeconds, double fps);

        // Write the last composed frame to a 32-bpp BMP. Returns false on I/O failure.
        bool DumpBmp(const std::wstring& path);

        // Directory (created on construction) where frames should be dumped, e.g.
        // C:\ProgramData\USBDisplay\frames, or a fallback temp dir.
        const std::wstring& OutputDir() const { return m_outputDir; }

        // The GPU texture holding the last composed frame (for CopyResource into the
        // OS-provided swap-chain surface). Null if texture creation failed.
        Microsoft::WRL::ComPtr<ID3D11Texture2D> SurfaceTexture() const { return m_texture; }

        uint32_t Width() const { return m_width; }
        uint32_t Height() const { return m_height; }

    private:
        void FillRect(int x0, int y0, int x1, int y1, uint32_t bgra);
        void DrawChar(int x, int y, int scale, char c, uint32_t bgra);
        void DrawText(int x, int y, int scale, const char* text, uint32_t bgra);
        static std::wstring ChooseOutputDir();

        uint32_t m_width;
        uint32_t m_height;
        std::vector<uint32_t> m_pixels; // BGRA, row-major (top-down), size = width*height
        std::wstring m_outputDir;
        Microsoft::WRL::ComPtr<ID3D11Device> m_device;
        Microsoft::WRL::ComPtr<ID3D11DeviceContext> m_context;
        Microsoft::WRL::ComPtr<ID3D11Texture2D> m_texture;
    };
}
