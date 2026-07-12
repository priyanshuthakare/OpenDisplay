#pragma once

#include <array>
#include <cstdint>
#include <windows.h>
#include <iddcx.h>

namespace UsbDisplay
{
    struct DisplayMode
    {
        uint32_t Width;
        uint32_t Height;
        uint32_t RefreshHz;
    };

    constexpr DisplayMode SupportedModes[] = {
        {1920, 1080, 60},
        {2560, 1440, 60},
        {2560, 1440, 120},
        {3840, 2160, 60},
    };

    constexpr uint32_t PreferredModeIndex = 0;

    std::array<uint8_t, 128> BuildEdid(uint32_t connectorIndex);
    IDDCX_MONITOR_MODE CreateMonitorMode(const DisplayMode& mode, IDDCX_MONITOR_MODE_ORIGIN origin);
    IDDCX_TARGET_MODE CreateTargetMode(const DisplayMode& mode);
}

