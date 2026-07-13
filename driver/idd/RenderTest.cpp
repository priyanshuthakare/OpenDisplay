#include "RenderTest.h"
#include "Trace.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>

using Microsoft::WRL::ComPtr;

namespace UsbDisplay
{
    namespace
    {
        constexpr uint32_t MakeBgra(uint8_t r, uint8_t g, uint8_t b, uint8_t a = 0xFF)
        {
            return (static_cast<uint32_t>(a) << 24) | (static_cast<uint32_t>(r) << 16) |
                   (static_cast<uint32_t>(g) << 8) | static_cast<uint32_t>(b);
        }

        // 5x7 bitmap font, columns LSB=top. Only the glyphs the readout needs.
        struct Glyph { char c; uint8_t col[5]; };
        const Glyph kFont[] = {
            {'0',{0x3E,0x51,0x49,0x45,0x3E}}, {'1',{0x00,0x42,0x7F,0x40,0x00}},
            {'2',{0x42,0x61,0x51,0x49,0x46}}, {'3',{0x21,0x41,0x45,0x4B,0x31}},
            {'4',{0x18,0x14,0x12,0x7F,0x10}}, {'5',{0x27,0x45,0x45,0x45,0x39}},
            {'6',{0x3C,0x4A,0x49,0x49,0x30}}, {'7',{0x01,0x71,0x09,0x05,0x03}},
            {'8',{0x36,0x49,0x49,0x49,0x36}}, {'9',{0x06,0x49,0x49,0x29,0x1E}},
            {'.',{0x00,0x60,0x60,0x00,0x00}}, {':',{0x00,0x36,0x36,0x00,0x00}},
            {' ',{0x00,0x00,0x00,0x00,0x00}}, {'F',{0x7F,0x09,0x09,0x09,0x01}},
            {'P',{0x7F,0x09,0x09,0x09,0x06}}, {'S',{0x26,0x49,0x49,0x49,0x32}},
            {'R',{0x7F,0x09,0x19,0x29,0x46}}, {'M',{0x7F,0x02,0x0C,0x02,0x7F}},
            {'E',{0x7F,0x49,0x49,0x49,0x41}}, {'=',{0x14,0x14,0x14,0x14,0x14}},
            {'X',{0x63,0x14,0x08,0x14,0x63}}, {'x',{0x63,0x14,0x08,0x14,0x63}},
        };
        const uint8_t* FindGlyph(char c)
        {
            for (const auto& g : kFont) { if (g.c == c) return g.col; }
            return nullptr;
        }
    }

    TestPatternRenderer::TestPatternRenderer(ComPtr<ID3D11Device> device,
                                             ComPtr<ID3D11DeviceContext> context,
                                             uint32_t width, uint32_t height)
        : m_width(width), m_height(height), m_pixels(static_cast<size_t>(width) * height, 0xFF000000),
          m_device(std::move(device)), m_context(std::move(context))
    {
        m_outputDir = ChooseOutputDir();
        USBLOG_INFO(L"TestPattern: renderer ready (%ux%u), frames -> %s", width, height, m_outputDir.c_str());
    }

    std::wstring TestPatternRenderer::ChooseOutputDir()
    {
        // Use plain kernel32 only (no shell32) so nothing extra is loaded into the
        // sandboxed UMDF host. %ProgramData%\USBDisplay\frames, created level by level.
        wchar_t expanded[MAX_PATH] = {};
        std::wstring dir;
        if (ExpandEnvironmentStringsW(L"%ProgramData%\\USBDisplay\\frames", expanded, MAX_PATH) > 0)
        {
            dir = expanded;
        }
        else
        {
            wchar_t tmp[MAX_PATH] = {};
            GetTempPathW(MAX_PATH, tmp);
            dir = std::wstring(tmp) + L"USBDisplay\\frames";
        }
        // Create each path component (CreateDirectoryW needs parents to exist).
        std::wstring partial;
        for (size_t i = 0; i < dir.size(); ++i)
        {
            partial += dir[i];
            if (dir[i] == L'\\' || i + 1 == dir.size())
            {
                if (partial.size() > 3) { CreateDirectoryW(partial.c_str(), nullptr); }
            }
        }
        return dir;
    }

    void TestPatternRenderer::FillRect(int x0, int y0, int x1, int y1, uint32_t bgra)
    {
        x0 = std::max(0, x0); y0 = std::max(0, y0);
        x1 = std::min<int>(m_width, x1); y1 = std::min<int>(m_height, y1);
        for (int y = y0; y < y1; ++y)
        {
            uint32_t* row = &m_pixels[static_cast<size_t>(y) * m_width];
            for (int x = x0; x < x1; ++x) { row[x] = bgra; }
        }
    }

    void TestPatternRenderer::DrawChar(int x, int y, int scale, char c, uint32_t bgra)
    {
        const uint8_t* col = FindGlyph(c);
        if (!col) return;
        for (int cx = 0; cx < 5; ++cx)
        {
            for (int cy = 0; cy < 7; ++cy)
            {
                if (col[cx] & (1 << cy))
                {
                    FillRect(x + cx * scale, y + cy * scale, x + (cx + 1) * scale, y + (cy + 1) * scale, bgra);
                }
            }
        }
    }

    void TestPatternRenderer::DrawText(int x, int y, int scale, const char* text, uint32_t bgra)
    {
        int cursor = x;
        for (const char* p = text; *p; ++p)
        {
            DrawChar(cursor, y, scale, *p, bgra);
            cursor += 6 * scale;
        }
    }

    void TestPatternRenderer::Render(uint64_t frameIndex, double elapsedSeconds, double fps)
    {
        const int w = static_cast<int>(m_width);
        const int h = static_cast<int>(m_height);

        // 1. Scrolling SMPTE-style vertical colour bars.
        const uint32_t bars[] = {
            MakeBgra(192,192,192), MakeBgra(192,192,0), MakeBgra(0,192,192),
            MakeBgra(0,192,0), MakeBgra(192,0,192), MakeBgra(192,0,0), MakeBgra(0,0,192)
        };
        const int barCount = static_cast<int>(sizeof(bars) / sizeof(bars[0]));
        const int barW = std::max(1, w / barCount);
        const int scroll = static_cast<int>(elapsedSeconds * 120.0) % barW;
        for (int x = 0; x < w; ++x)
        {
            int idx = ((x + scroll) / barW) % barCount;
            uint32_t c = bars[idx];
            for (int y = 0; y < h * 3 / 4; ++y) { m_pixels[static_cast<size_t>(y) * m_width + x] = c; }
        }

        // 2. Moving horizontal RGB gradient band (bottom quarter).
        const int bandY0 = h * 3 / 4;
        const double phase = std::fmod(elapsedSeconds * 0.5, 1.0);
        for (int y = bandY0; y < h; ++y)
        {
            for (int x = 0; x < w; ++x)
            {
                double t = std::fmod(static_cast<double>(x) / w + phase, 1.0);
                uint8_t r = static_cast<uint8_t>(255 * std::fabs(std::sin(t * 3.14159)));
                uint8_t g = static_cast<uint8_t>(255 * std::fabs(std::sin((t + 0.33) * 3.14159)));
                uint8_t b = static_cast<uint8_t>(255 * std::fabs(std::sin((t + 0.66) * 3.14159)));
                m_pixels[static_cast<size_t>(y) * m_width + x] = MakeBgra(r, g, b);
            }
        }

        // 3. Bouncing square.
        const int sq = std::max(32, h / 12);
        const int travelX = std::max(1, w - sq);
        const int travelY = std::max(1, (h * 3 / 4) - sq);
        auto tri = [](double v) { v = std::fmod(v, 2.0); return v < 1.0 ? v : 2.0 - v; };
        int sx = static_cast<int>(tri(elapsedSeconds * 0.7) * travelX);
        int sy = static_cast<int>(tri(elapsedSeconds * 0.9) * travelY);
        FillRect(sx, sy, sx + sq, sy + sq, MakeBgra(255, 255, 255));
        FillRect(sx + 3, sy + 3, sx + sq - 3, sy + sq - 3, MakeBgra(0, 0, 0));

        // 4. FPS + frame counter readout (top-left, on a dark plate).
        char line1[64]; char line2[64]; char line3[64];
        std::snprintf(line1, sizeof(line1), "USBDISPLAY");
        std::snprintf(line2, sizeof(line2), "FPS=%d.%d", (int)fps, (int)(fps * 10) % 10);
        std::snprintf(line3, sizeof(line3), "FRAME=%llu", (unsigned long long)frameIndex);
        const int scale = std::max(2, w / 480);
        FillRect(8, 8, 8 + 46 * 6 * scale / 4, 8 + 3 * 9 * scale, MakeBgra(0, 0, 0));
        DrawText(12, 12, scale, line1, MakeBgra(0, 255, 0));
        DrawText(12, 12 + 9 * scale, scale, line2, MakeBgra(0, 255, 0));
        DrawText(12, 12 + 18 * scale, scale, line3, MakeBgra(0, 255, 0));

        // The composed frame lives in m_pixels (CPU). It is dumped to BMP by the
        // caller as deterministic visual proof; no GPU upload is needed for that and
        // keeping this path CPU-only avoids any D3D lifetime hazards in the host.
    }

    bool TestPatternRenderer::DumpBmp(const std::wstring& path)
    {
#pragma pack(push, 1)
        struct BmpFileHeader { uint16_t bfType; uint32_t bfSize; uint16_t r1, r2; uint32_t bfOffBits; };
        struct BmpInfoHeader { uint32_t biSize; int32_t biWidth; int32_t biHeight; uint16_t biPlanes;
                               uint16_t biBitCount; uint32_t biCompression; uint32_t biSizeImage;
                               int32_t x, y; uint32_t clrUsed, clrImportant; };
#pragma pack(pop)
        const uint32_t imgSize = m_width * m_height * 4;
        BmpFileHeader fh = { 0x4D42, static_cast<uint32_t>(sizeof(fh) + sizeof(BmpInfoHeader) + imgSize), 0, 0,
                             sizeof(fh) + sizeof(BmpInfoHeader) };
        BmpInfoHeader ih = { sizeof(BmpInfoHeader), static_cast<int32_t>(m_width),
                             -static_cast<int32_t>(m_height), 1, 32, 0 /*BI_RGB*/, imgSize, 2835, 2835, 0, 0 };

        FILE* f = nullptr;
        if (_wfopen_s(&f, path.c_str(), L"wb") != 0 || !f) { return false; }
        fwrite(&fh, sizeof(fh), 1, f);
        fwrite(&ih, sizeof(ih), 1, f);
        fwrite(m_pixels.data(), imgSize, 1, f);
        fclose(f);
        return true;
    }
}
