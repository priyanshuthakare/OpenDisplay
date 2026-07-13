#pragma once

// Runtime diagnostics for the USBDisplay indirect display driver.
//
// Every important callback emits a structured TraceLogging (ETW) event AND a
// mirrored OutputDebugStringW line so failures are immediately visible in
// Sysinternals DebugView without any ETW capture setup.
//
// Capture the ETW provider live with:
//   tracelog -start UsbDisplay -guid #B9A2F0C4-3E7D-4C1A-9F2B-7A6E5D4C3B21 -f usbdisplay.etl -flags 0xff -level 5
//   tracefmt usbdisplay.etl        (or open in WPA)
// Or simply run DebugView (with "Capture Global Win32") to see the mirrored lines.

#include <windows.h>
#include <TraceLoggingProvider.h>
#include <winmeta.h>

TRACELOGGING_DECLARE_PROVIDER(g_UsbDisplayTraceProvider);

namespace UsbDisplay
{
    // Registers/unregisters the TraceLogging provider. Register once in
    // DriverEntry; unregister when the driver object is cleaned up.
    void TraceRegister();
    void TraceUnregister();

    // Emits a preformatted message to both ETW and the debugger.
    void TraceEmit(UCHAR level, PCWSTR function, PCWSTR message);

    // printf-style front end used by the USBLOG_* macros.
    void TraceEmitf(UCHAR level, PCWSTR function, _Printf_format_string_ PCWSTR format, ...);
}

// __VA_ARGS__ always contains at least the format string, so there is never a
// dangling comma to worry about across preprocessor modes.
#define USBLOG_ERROR(...) ::UsbDisplay::TraceEmitf(WINEVENT_LEVEL_ERROR,   __FUNCTIONW__, __VA_ARGS__)
#define USBLOG_WARN(...)  ::UsbDisplay::TraceEmitf(WINEVENT_LEVEL_WARNING, __FUNCTIONW__, __VA_ARGS__)
#define USBLOG_INFO(...)  ::UsbDisplay::TraceEmitf(WINEVENT_LEVEL_INFO,    __FUNCTIONW__, __VA_ARGS__)
