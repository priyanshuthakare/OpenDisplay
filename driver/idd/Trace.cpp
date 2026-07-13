#include "Trace.h"

#include <cstdarg>
#include <cwchar>

// {B9A2F0C4-3E7D-4C1A-9F2B-7A6E5D4C3B21}
TRACELOGGING_DEFINE_PROVIDER(
    g_UsbDisplayTraceProvider,
    "USBDisplay.IddDriver",
    (0xb9a2f0c4, 0x3e7d, 0x4c1a, 0x9f, 0x2b, 0x7a, 0x6e, 0x5d, 0x4c, 0x3b, 0x21));

namespace UsbDisplay
{
    void TraceRegister()
    {
        TraceLoggingRegister(g_UsbDisplayTraceProvider);
    }

    void TraceUnregister()
    {
        TraceLoggingUnregister(g_UsbDisplayTraceProvider);
    }

    void TraceEmit(UCHAR level, PCWSTR function, PCWSTR message)
    {
        // TraceLoggingLevel() requires a compile-time constant, so dispatch to a
        // fixed-level write per severity to keep ETW level filtering usable.
#define USBDISPLAY_TL_WRITE(lvl) \
        TraceLoggingWrite( \
            g_UsbDisplayTraceProvider, \
            "UsbDisplayLog", \
            TraceLoggingLevel(lvl), \
            TraceLoggingWideString(function, "Function"), \
            TraceLoggingWideString(message, "Message"))

        switch (level)
        {
        case WINEVENT_LEVEL_ERROR:   USBDISPLAY_TL_WRITE(WINEVENT_LEVEL_ERROR);   break;
        case WINEVENT_LEVEL_WARNING: USBDISPLAY_TL_WRITE(WINEVENT_LEVEL_WARNING); break;
        default:                     USBDISPLAY_TL_WRITE(WINEVENT_LEVEL_INFO);    break;
        }
#undef USBDISPLAY_TL_WRITE

        // Mirror to the debugger so DebugView shows every event with no ETW setup.
        wchar_t line[640];
        _snwprintf_s(line, ARRAYSIZE(line), _TRUNCATE, L"[USBDisplay] %s: %s\n", function, message);
        OutputDebugStringW(line);
    }

    void TraceEmitf(UCHAR level, PCWSTR function, PCWSTR format, ...)
    {
        wchar_t buffer[512];
        va_list args;
        va_start(args, format);
        _vsnwprintf_s(buffer, ARRAYSIZE(buffer), _TRUNCATE, format, args);
        va_end(args);
        TraceEmit(level, function, buffer);
    }
}
