#include "Driver.h"
#include "Device.h"
#include "Edid.h"

#include <algorithm>

using namespace UsbDisplay;

extern "C" BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void* reserved)
{
    UNREFERENCED_PARAMETER(instance);
    UNREFERENCED_PARAMETER(reason);
    UNREFERENCED_PARAMETER(reserved);
    return TRUE;
}

extern "C" NTSTATUS DriverEntry(PDRIVER_OBJECT driverObject, PUNICODE_STRING registryPath)
{
    WDF_DRIVER_CONFIG config;
    WDF_OBJECT_ATTRIBUTES attributes;

    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);
    WDF_DRIVER_CONFIG_INIT(&config, UsbDisplayEvtDeviceAdd);

    return WdfDriverCreate(driverObject, registryPath, &attributes, &config, WDF_NO_HANDLE);
}

NTSTATUS UsbDisplayEvtDeviceAdd(WDFDRIVER driver, PWDFDEVICE_INIT deviceInit)
{
    UNREFERENCED_PARAMETER(driver);

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
        return status;
    }

    WDF_OBJECT_ATTRIBUTES attributes;
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, DeviceContext);
    attributes.EvtCleanupCallback = [](WDFOBJECT object)
    {
        auto* context = WdfObjectGet_DeviceContext(object);
        delete context->Instance;
        context->Instance = nullptr;
    };

    WDFDEVICE device = nullptr;
    status = WdfDeviceCreate(&deviceInit, &attributes, &device);
    if (!NT_SUCCESS(status))
    {
        return status;
    }

    status = IddCxDeviceInitialize(device);
    if (!NT_SUCCESS(status))
    {
        return status;
    }

    auto* context = WdfObjectGet_DeviceContext(device);
    context->Instance = new Device(device);
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtDeviceD0Entry(WDFDEVICE device, WDF_POWER_DEVICE_STATE previousState)
{
    UNREFERENCED_PARAMETER(previousState);
    auto* context = WdfObjectGet_DeviceContext(device);
    return context->Instance->InitializeAdapter();
}

NTSTATUS UsbDisplayEvtDeviceD0Exit(WDFDEVICE device, WDF_POWER_DEVICE_STATE targetState)
{
    UNREFERENCED_PARAMETER(targetState);
    auto* context = WdfObjectGet_DeviceContext(device);
    context->Instance->RemoveAllMonitors();
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtAdapterInitFinished(IDDCX_ADAPTER adapter, const IDARG_IN_ADAPTER_INIT_FINISHED* args)
{
    auto* context = WdfObjectGet_DeviceContext(adapter);
    return context->Instance->AdapterInitFinished(args->AdapterInitStatus);
}

NTSTATUS UsbDisplayEvtAdapterCommitModes(IDDCX_ADAPTER adapter, const IDARG_IN_COMMITMODES* args)
{
    auto* context = WdfObjectGet_DeviceContext(adapter);
    return context->Instance->CommitModes(args);
}

NTSTATUS UsbDisplayEvtParseMonitorDescription(const IDARG_IN_PARSEMONITORDESCRIPTION* inArgs, IDARG_OUT_PARSEMONITORDESCRIPTION* outArgs)
{
    UNREFERENCED_PARAMETER(inArgs);
    outArgs->MonitorModeBufferOutputCount = ARRAYSIZE(SupportedModes);
    if (inArgs->MonitorModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }
    if (inArgs->MonitorModeBufferInputCount < ARRAYSIZE(SupportedModes))
    {
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
    outArgs->DefaultMonitorModeBufferOutputCount = ARRAYSIZE(SupportedModes);
    if (inArgs->DefaultMonitorModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }
    if (inArgs->DefaultMonitorModeBufferInputCount < ARRAYSIZE(SupportedModes))
    {
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
    outArgs->TargetModeBufferOutputCount = ARRAYSIZE(SupportedModes);
    if (inArgs->TargetModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }
    if (inArgs->TargetModeBufferInputCount < ARRAYSIZE(SupportedModes))
    {
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
    auto* context = WdfObjectGet_IndirectMonitorContext(monitor);
    context->Monitor->AssignSwapChain(args->hSwapChain, args->RenderAdapterLuid, args->hNextSurfaceAvailable);
    return STATUS_SUCCESS;
}

NTSTATUS UsbDisplayEvtMonitorUnassignSwapChain(IDDCX_MONITOR monitor)
{
    auto* context = WdfObjectGet_IndirectMonitorContext(monitor);
    context->Monitor->UnassignSwapChain();
    return STATUS_SUCCESS;
}
