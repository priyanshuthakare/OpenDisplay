#include "Driver.h"
#include "Device.h"
#include "Edid.h"
#include "Trace.h"

#include <algorithm>

using namespace UsbDisplay;

extern "C" BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void* reserved)
{
    UNREFERENCED_PARAMETER(instance);
    UNREFERENCED_PARAMETER(reserved);
    // Earliest possible signal: fires on LoadLibrary into WUDFHost, before the WDF
    // stub calls DriverEntry. Uses OutputDebugStringW directly (no ETW/CRT state) so
    // it is visible in DebugView / a global OutputDebugString capture even if WDF
    // version-bind later fails and DriverEntry is never reached.
    switch (reason)
    {
    case DLL_PROCESS_ATTACH:
        OutputDebugStringW(L"[USBDisplay] DllMain: DLL_PROCESS_ATTACH (module loaded into host)\n");
        break;
    case DLL_PROCESS_DETACH:
        OutputDebugStringW(L"[USBDisplay] DllMain: DLL_PROCESS_DETACH (module unloading)\n");
        break;
    default:
        break;
    }
    return TRUE;
}

// Unregisters the TraceLogging provider when the framework tears the driver down.
static void UsbDisplayEvtDriverCleanup(WDFOBJECT driverObject)
{
    UNREFERENCED_PARAMETER(driverObject);
    USBLOG_INFO(L"Driver cleanup; unregistering trace provider");
    TraceUnregister();
}

extern "C" NTSTATUS DriverEntry(PDRIVER_OBJECT driverObject, PUNICODE_STRING registryPath)
{
    // Raw signal before any provider/CRT setup: proves WDF version-bind succeeded
    // and the framework actually called our DriverEntry.
    OutputDebugStringW(L"[USBDisplay] DriverEntry: reached (WDF bind OK)\n");

    TraceRegister();
    USBLOG_INFO(L"DriverEntry: enter (IddCx USBDisplay virtual display driver)");

    WDF_DRIVER_CONFIG config;
    WDF_OBJECT_ATTRIBUTES attributes;

    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);
    attributes.EvtCleanupCallback = UsbDisplayEvtDriverCleanup;
    WDF_DRIVER_CONFIG_INIT(&config, UsbDisplayEvtDeviceAdd);

    NTSTATUS status = WdfDriverCreate(driverObject, registryPath, &attributes, &config, WDF_NO_HANDLE);
    if (!NT_SUCCESS(status))
    {
        USBLOG_ERROR(L"DriverEntry: WdfDriverCreate failed 0x%08X", status);
        TraceUnregister();
        return status;
    }

    USBLOG_INFO(L"DriverEntry: WdfDriverCreate succeeded");
    return status;
}

NTSTATUS UsbDisplayEvtDeviceAdd(WDFDRIVER driver, PWDFDEVICE_INIT deviceInit)
{
    UNREFERENCED_PARAMETER(driver);
    USBLOG_INFO(L"DeviceAdd: enter");

    WDF_PNPPOWER_EVENT_CALLBACKS powerCallbacks;
    WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&powerCallbacks);
    powerCallbacks.EvtDeviceD0Entry = UsbDisplayEvtDeviceD0Entry;
    powerCallbacks.EvtDeviceD0Exit = UsbDisplayEvtDeviceD0Exit;
    WdfDeviceInitSetPnpPowerEventCallbacks(deviceInit, &powerCallbacks);

    IDD_CX_CLIENT_CONFIG iddConfig;
    IDD_CX_CLIENT_CONFIG_INIT(&iddConfig);
    iddConfig.EvtIddCxAdapterInitFinished = UsbDisplayEvtAdapterInitFinished;
    iddConfig.EvtIddCxAdapterCommitModes = UsbDisplayEvtAdapterCommitModes;
    iddConfig.EvtIddCxParseMonitorDescription = UsbDisplayEvtParseMonitorDescription;
    iddConfig.EvtIddCxMonitorGetDefaultDescriptionModes = UsbDisplayEvtMonitorGetDefaultModes;
    iddConfig.EvtIddCxMonitorQueryTargetModes = UsbDisplayEvtMonitorQueryTargetModes;
    iddConfig.EvtIddCxMonitorAssignSwapChain = UsbDisplayEvtMonitorAssignSwapChain;
    iddConfig.EvtIddCxMonitorUnassignSwapChain = UsbDisplayEvtMonitorUnassignSwapChain;

    NTSTATUS status = IddCxDeviceInitConfig(deviceInit, &iddConfig);
    if (!NT_SUCCESS(status))
    {
        USBLOG_ERROR(L"DeviceAdd: IddCxDeviceInitConfig failed 0x%08X", status);
        return status;
    }

    WDF_OBJECT_ATTRIBUTES attributes;
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, DeviceContext);
    attributes.EvtCleanupCallback = [](WDFOBJECT object)
    {
        auto* context = WdfObjectGet_DeviceContext(object);
        USBLOG_INFO(L"DeviceCleanup: destroying Device instance");
        delete context->Instance;
        context->Instance = nullptr;
    };

    WDFDEVICE device = nullptr;
    status = WdfDeviceCreate(&deviceInit, &attributes, &device);
    if (!NT_SUCCESS(status))
    {
        USBLOG_ERROR(L"DeviceAdd: WdfDeviceCreate failed 0x%08X", status);
        return status;
    }

    status = IddCxDeviceInitialize(device);
    if (!NT_SUCCESS(status))
    {
        USBLOG_ERROR(L"DeviceAdd: IddCxDeviceInitialize failed 0x%08X", status);
        return status;
    }

    auto* context = WdfObjectGet_DeviceContext(device);
    context->Instance = new Device(device);
    USBLOG_INFO(L"DeviceAdd: success; Device instance created");
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtDeviceD0Entry(WDFDEVICE device, WDF_POWER_DEVICE_STATE previousState)
{
    USBLOG_INFO(L"D0Entry: enter (previousState=%d)", previousState);
    auto* context = WdfObjectGet_DeviceContext(device);
    NTSTATUS status = context->Instance->InitializeAdapter();
    USBLOG_INFO(L"D0Entry: InitializeAdapter returned 0x%08X", status);
    return status;
}

NTSTATUS UsbDisplayEvtDeviceD0Exit(WDFDEVICE device, WDF_POWER_DEVICE_STATE targetState)
{
    USBLOG_INFO(L"D0Exit: enter (targetState=%d)", targetState);
    auto* context = WdfObjectGet_DeviceContext(device);
    context->Instance->RemoveAllMonitors();
    USBLOG_INFO(L"D0Exit: monitors removed");
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtAdapterInitFinished(IDDCX_ADAPTER adapter, const IDARG_IN_ADAPTER_INIT_FINISHED* args)
{
    USBLOG_INFO(L"AdapterInitFinished: enter (AdapterInitStatus=0x%08X)", args->AdapterInitStatus);
    auto* context = WdfObjectGet_DeviceContext(adapter);
    NTSTATUS status = context->Instance->AdapterInitFinished(args->AdapterInitStatus);
    USBLOG_INFO(L"AdapterInitFinished: CreateMonitor path returned 0x%08X", status);
    return status;
}

NTSTATUS UsbDisplayEvtAdapterCommitModes(IDDCX_ADAPTER adapter, const IDARG_IN_COMMITMODES* args)
{
    USBLOG_INFO(L"CommitModes: enter (PathCount=%u)", args->PathCount);
    auto* context = WdfObjectGet_DeviceContext(adapter);
    return context->Instance->CommitModes(args);
}

NTSTATUS UsbDisplayEvtParseMonitorDescription(const IDARG_IN_PARSEMONITORDESCRIPTION* inArgs, IDARG_OUT_PARSEMONITORDESCRIPTION* outArgs)
{
    USBLOG_INFO(L"ParseMonitorDescription: enter (InputCount=%u)", inArgs->MonitorModeBufferInputCount);
    outArgs->MonitorModeBufferOutputCount = ARRAYSIZE(SupportedModes);
    if (inArgs->MonitorModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }
    if (inArgs->MonitorModeBufferInputCount < ARRAYSIZE(SupportedModes))
    {
        USBLOG_WARN(L"ParseMonitorDescription: buffer too small (need %u)", (UINT)ARRAYSIZE(SupportedModes));
        return STATUS_BUFFER_TOO_SMALL;
    }

    for (UINT i = 0; i < ARRAYSIZE(SupportedModes); ++i)
    {
        inArgs->pMonitorModes[i] = CreateMonitorMode(SupportedModes[i], IDDCX_MONITOR_MODE_ORIGIN_MONITORDESCRIPTOR);
    }
    outArgs->PreferredMonitorModeIdx = PreferredModeIndex;
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtMonitorGetDefaultModes(IDDCX_MONITOR monitor, const IDARG_IN_GETDEFAULTDESCRIPTIONMODES* inArgs, IDARG_OUT_GETDEFAULTDESCRIPTIONMODES* outArgs)
{
    UNREFERENCED_PARAMETER(monitor);
    USBLOG_INFO(L"GetDefaultModes: enter (InputCount=%u)", inArgs->DefaultMonitorModeBufferInputCount);
    outArgs->DefaultMonitorModeBufferOutputCount = ARRAYSIZE(SupportedModes);
    if (inArgs->DefaultMonitorModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }
    if (inArgs->DefaultMonitorModeBufferInputCount < ARRAYSIZE(SupportedModes))
    {
        USBLOG_WARN(L"GetDefaultModes: buffer too small (need %u)", (UINT)ARRAYSIZE(SupportedModes));
        return STATUS_BUFFER_TOO_SMALL;
    }

    for (UINT i = 0; i < ARRAYSIZE(SupportedModes); ++i)
    {
        inArgs->pDefaultMonitorModes[i] = CreateMonitorMode(SupportedModes[i], IDDCX_MONITOR_MODE_ORIGIN_DRIVER);
    }
    outArgs->PreferredMonitorModeIdx = PreferredModeIndex;
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtMonitorQueryTargetModes(IDDCX_MONITOR monitor, const IDARG_IN_QUERYTARGETMODES* inArgs, IDARG_OUT_QUERYTARGETMODES* outArgs)
{
    UNREFERENCED_PARAMETER(monitor);
    USBLOG_INFO(L"QueryTargetModes: enter (InputCount=%u)", inArgs->TargetModeBufferInputCount);
    outArgs->TargetModeBufferOutputCount = ARRAYSIZE(SupportedModes);
    if (inArgs->TargetModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }
    if (inArgs->TargetModeBufferInputCount < ARRAYSIZE(SupportedModes))
    {
        USBLOG_WARN(L"QueryTargetModes: buffer too small (need %u)", (UINT)ARRAYSIZE(SupportedModes));
        return STATUS_BUFFER_TOO_SMALL;
    }

    for (UINT i = 0; i < ARRAYSIZE(SupportedModes); ++i)
    {
        inArgs->pTargetModes[i] = CreateTargetMode(SupportedModes[i]);
    }
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtMonitorAssignSwapChain(IDDCX_MONITOR monitor, const IDARG_IN_SETSWAPCHAIN* args)
{
    USBLOG_INFO(L"AssignSwapChain: enter (RenderAdapterLuid=%08X:%08X)",
        args->RenderAdapterLuid.HighPart, args->RenderAdapterLuid.LowPart);
    auto* context = WdfObjectGet_IndirectMonitorContext(monitor);
    context->Monitor->AssignSwapChain(args->hSwapChain, args->RenderAdapterLuid, args->hNextSurfaceAvailable);
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtMonitorUnassignSwapChain(IDDCX_MONITOR monitor)
{
    USBLOG_INFO(L"UnassignSwapChain: enter");
    auto* context = WdfObjectGet_IndirectMonitorContext(monitor);
    context->Monitor->UnassignSwapChain();
    return STATUS_SUCCESS;
}
