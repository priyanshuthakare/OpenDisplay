#include "Edid.h"

#include <algorithm>
#include <cstring>

namespace
{
    constexpr uint16_t VendorId(char a, char b, char c)
    {
        return static_cast<uint16_t>(((a - '@') & 0x1f) << 10 |
                                     ((b - '@') & 0x1f) << 5 |
                                     ((c - '@') & 0x1f));
    }

    void FillDetailedTiming(uint8_t* descriptor, uint32_t width, uint32_t height, uint32_t refreshHz)
    {
        const uint32_t hBlank = width >= 3840 ? 560 : 280;
        const uint32_t vBlank = height >= 2160 ? 90 : 45;
        const uint32_t hTotal = width + hBlank;
        const uint32_t vTotal = height + vBlank;
        const uint32_t pixelClock10Khz = (hTotal * vTotal * refreshHz) / 10000;

        std::memset(descriptor, 0, 18);
        descriptor[0] = static_cast<uint8_t>(pixelClock10Khz & 0xff);
        descriptor[1] = static_cast<uint8_t>((pixelClock10Khz >> 8) & 0xff);
        descriptor[2] = static_cast<uint8_t>(width & 0xff);
        descriptor[3] = static_cast<uint8_t>(hBlank & 0xff);
        descriptor[4] = static_cast<uint8_t>(((width >> 8) & 0x0f) << 4 | ((hBlank >> 8) & 0x0f));
        descriptor[5] = static_cast<uint8_t>(height & 0xff);
        descriptor[6] = static_cast<uint8_t>(vBlank & 0xff);
        descriptor[7] = static_cast<uint8_t>(((height >> 8) & 0x0f) << 4 | ((vBlank >> 8) & 0x0f));
        descriptor[8] = 48;
        descriptor[9] = 32;
        descriptor[10] = 32;
        descriptor[11] = 5;
        descriptor[12] = 0;
        descriptor[13] = 0;
        descriptor[14] = 0;
        descriptor[15] = 0;
        descriptor[16] = 0;
        descriptor[17] = 0x1e;
    }

    void FillTextDescriptor(uint8_t* descriptor, uint8_t tag, const char* text)
    {
        std::memset(descriptor, 0, 18);
        descriptor[3] = tag;
        descriptor[4] = 0;

        const size_t length = std::min<size_t>(13, std::strlen(text));
        std::memcpy(&descriptor[5], text, length);
        for (size_t i = length; i < 13; ++i)
        {
            descriptor[5 + i] = ' ';
        }
        descriptor[17] = '\n';
    }

    void FillRangeDescriptor(uint8_t* descriptor)
    {
        std::memset(descriptor, 0, 18);
        descriptor[3] = 0xfd;
        descriptor[5] = 30;
        descriptor[6] = 120;
        descriptor[7] = 30;
        descriptor[8] = 160;
        descriptor[9] = 0xff;
    }

    void UpdateChecksum(std::array<uint8_t, 128>& edid)
    {
        uint8_t sum = 0;
        for (size_t i = 0; i < 127; ++i)
        {
            sum = static_cast<uint8_t>(sum + edid[i]);
        }
        edid[127] = static_cast<uint8_t>(0u - sum);
    }

    void FillSignalInfo(DISPLAYCONFIG_VIDEO_SIGNAL_INFO& signal, uint32_t width, uint32_t height, uint32_t refreshHz, bool monitorMode)
    {
        signal.totalSize.cx = signal.activeSize.cx = width;
        signal.totalSize.cy = signal.activeSize.cy = height;
        signal.AdditionalSignalInfo.vSyncFreqDivider = monitorMode ? 0 : 1;
        signal.AdditionalSignalInfo.videoStandard = 255;
        signal.vSyncFreq.Numerator = refreshHz;
        signal.vSyncFreq.Denominator = 1;
        signal.hSyncFreq.Numerator = refreshHz * height;
        signal.hSyncFreq.Denominator = 1;
        signal.scanLineOrdering = DISPLAYCONFIG_SCANLINE_ORDERING_PROGRESSIVE;
        signal.pixelRate = static_cast<UINT64>(refreshHz) * width * height;
    }
}

namespace UsbDisplay
{
    std::array<uint8_t, 128> BuildEdid(uint32_t connectorIndex)
    {
        std::array<uint8_t, 128> edid = {};
        const uint8_t header[] = {0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00};
        std::copy(std::begin(header), std::end(header), edid.begin());

        const uint16_t vendor = VendorId('U', 'S', 'B');
        edid[8] = static_cast<uint8_t>((vendor >> 8) & 0xff);
        edid[9] = static_cast<uint8_t>(vendor & 0xff);
        edid[10] = 0x01;
        edid[11] = 0xd1;
        edid[12] = static_cast<uint8_t>(connectorIndex + 1);
        edid[13] = 0;
        edid[14] = 0;
        edid[15] = 0;
        edid[16] = 1;
        edid[17] = 34;
        edid[18] = 1;
        edid[19] = 4;
        edid[20] = 0xa5;
        edid[21] = 34;
        edid[22] = 19;
        edid[23] = 120;
        edid[24] = 0x0a;
        edid[25] = 0xee;
        edid[26] = 0x95;
        edid[27] = 0xa3;
        edid[28] = 0x54;
        edid[29] = 0x4c;
        edid[30] = 0x99;
        edid[31] = 0x26;
        edid[32] = 0x0f;
        edid[33] = 0x50;
        edid[34] = 0x54;
        edid[35] = 0x21;
        edid[36] = 0x08;
        edid[37] = 0x00;

        FillDetailedTiming(&edid[54], 1920, 1080, 60);
        FillTextDescriptor(&edid[72], 0xff, "USBDisplay001");
        FillTextDescriptor(&edid[90], 0xfc, "USBDisplay");
        FillRangeDescriptor(&edid[108]);
        UpdateChecksum(edid);
        return edid;
    }

    IDDCX_MONITOR_MODE CreateMonitorMode(const DisplayMode& mode, IDDCX_MONITOR_MODE_ORIGIN origin)
    {
        IDDCX_MONITOR_MODE monitorMode = {};
        monitorMode.Size = sizeof(monitorMode);
        monitorMode.Origin = origin;
        FillSignalInfo(monitorMode.MonitorVideoSignalInfo, mode.Width, mode.Height, mode.RefreshHz, true);
        return monitorMode;
    }

    IDDCX_TARGET_MODE CreateTargetMode(const DisplayMode& mode)
    {
        IDDCX_TARGET_MODE targetMode = {};
        targetMode.Size = sizeof(targetMode);
        FillSignalInfo(targetMode.TargetVideoSignalInfo.targetVideoSignalInfo, mode.Width, mode.Height, mode.RefreshHz, false);
        return targetMode;
    }
}

