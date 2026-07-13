#include "Device.h"
#include "Trace.h"

#include <algorithm>
#include <objbase.h>

namespace UsbDisplay
{
    Device::Device(WDFDEVICE device) : m_wdfDevice(device)
    {
    }

    Device::~Device()
    {
        RemoveAllMonitors();
    }

    NTSTATUS Device::InitializeAdapter()
    {
        // The adapter is created once for the lifetime of the device. D0Entry can
        // fire again after a D0Exit (e.g. power transitions); creating a second
        // adapter would leak the first and confuse IddCx, so guard against it.
        if (m_adapterInitStarted)
        {
            USBLOG_INFO(L"InitializeAdapter: adapter already initialized; skipping");
            return STATUS_SUCCESS;
        }

        IDDCX_ADAPTER_CAPS caps = {};
        caps.Size = sizeof(caps);
        caps.MaxMonitorsSupported = MaxVirtualMonitors;
        caps.EndPointDiagnostics.Size = sizeof(caps.EndPointDiagnostics);
        caps.EndPointDiagnostics.GammaSupport = IDDCX_FEATURE_IMPLEMENTATION_NONE;
        caps.EndPointDiagnostics.TransmissionType = IDDCX_TRANSMISSION_TYPE_WIRED_OTHER;
        caps.EndPointDiagnostics.pEndPointFriendlyName = L"USBDisplay";
        caps.EndPointDiagnostics.pEndPointManufacturerName = L"USBDisplay";
        caps.EndPointDiagnostics.pEndPointModelName = L"USBDisplay Virtual Display Adapter";

        IDDCX_ENDPOINT_VERSION version = {};
        version.Size = sizeof(version);
        version.MajorVer = 1;
        version.MinorVer = 0;
        caps.EndPointDiagnostics.pFirmwareVersion = &version;
        caps.EndPointDiagnostics.pHardwareVersion = &version;

        WDF_OBJECT_ATTRIBUTES attributes;
        WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, DeviceContext);

        IDARG_IN_ADAPTER_INIT adapterInit = {};
        adapterInit.WdfDevice = m_wdfDevice;
        adapterInit.pCaps = &caps;
        adapterInit.ObjectAttributes = &attributes;

        IDARG_OUT_ADAPTER_INIT adapterInitOut = {};
        const NTSTATUS status = IddCxAdapterInitAsync(&adapterInit, &adapterInitOut);
        if (NT_SUCCESS(status))
        {
            m_adapterInitStarted = true;
            m_adapter = adapterInitOut.AdapterObject;
            auto* context = WdfObjectGet_DeviceContext(m_adapter);
            context->Instance = this;
            USBLOG_INFO(L"InitializeAdapter: IddCxAdapterInitAsync started (adapter=%p)", m_adapter);
        }
        else
        {
            USBLOG_ERROR(L"InitializeAdapter: IddCxAdapterInitAsync failed 0x%08X", status);
        }
        return status;
    }

    NTSTATUS Device::AdapterInitFinished(NTSTATUS initStatus)
    {
        if (!NT_SUCCESS(initStatus))
        {
            USBLOG_ERROR(L"AdapterInitFinished: adapter init failed 0x%08X", initStatus);
            return initStatus;
        }

        USBLOG_INFO(L"AdapterInitFinished: adapter ready; creating monitor 0");
        return CreateMonitor(0);
    }

    NTSTATUS Device::CommitModes(const IDARG_IN_COMMITMODES* args)
    {
        UNREFERENCED_PARAMETER(args);
        return STATUS_SUCCESS;
    }

    NTSTATUS Device::CreateMonitor(uint32_t connectorIndex)
    {
        if (!m_adapter || connectorIndex >= MaxVirtualMonitors)
        {
            return STATUS_INVALID_PARAMETER;
        }

        if (m_monitorHandles[connectorIndex] != nullptr)
        {
            return STATUS_SUCCESS;
        }

        auto edid = BuildEdid(connectorIndex);

        IDDCX_MONITOR_INFO monitorInfo = {};
        monitorInfo.Size = sizeof(monitorInfo);
        monitorInfo.MonitorType = DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI;
        monitorInfo.ConnectorIndex = connectorIndex;
        monitorInfo.MonitorDescription.Size = sizeof(monitorInfo.MonitorDescription);
        monitorInfo.MonitorDescription.Type = IDDCX_MONITOR_DESCRIPTION_TYPE_EDID;
        monitorInfo.MonitorDescription.DataSize = static_cast<UINT>(edid.size());
        monitorInfo.MonitorDescription.pData = edid.data();
        CoCreateGuid(&monitorInfo.MonitorContainerId);

        WDF_OBJECT_ATTRIBUTES attributes;
        WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, IndirectMonitorContext);
        attributes.EvtCleanupCallback = [](WDFOBJECT object)
        {
            auto* wrapper = WdfObjectGet_IndirectMonitorContext(object);
            delete wrapper->Monitor;
            wrapper->Monitor = nullptr;
        };

        IDARG_IN_MONITORCREATE create = {};
        create.ObjectAttributes = &attributes;
        create.pMonitorInfo = &monitorInfo;

        IDARG_OUT_MONITORCREATE createOut = {};
        NTSTATUS status = IddCxMonitorCreate(m_adapter, &create, &createOut);
        if (!NT_SUCCESS(status))
        {
            USBLOG_ERROR(L"CreateMonitor: IddCxMonitorCreate failed 0x%08X (connector=%u)", status, connectorIndex);
            return status;
        }

        auto* wrapper = WdfObjectGet_IndirectMonitorContext(createOut.MonitorObject);
        wrapper->Monitor = new IndirectMonitor(createOut.MonitorObject);
        m_monitorHandles[connectorIndex] = createOut.MonitorObject;
        USBLOG_INFO(L"CreateMonitor: monitor created (connector=%u, monitor=%p); signaling arrival",
            connectorIndex, createOut.MonitorObject);

        IDARG_OUT_MONITORARRIVAL arrival = {};
        status = IddCxMonitorArrival(createOut.MonitorObject, &arrival);
        if (!NT_SUCCESS(status))
        {
            USBLOG_ERROR(L"CreateMonitor: IddCxMonitorArrival failed 0x%08X (connector=%u)", status, connectorIndex);
            WdfObjectDelete(reinterpret_cast<WDFOBJECT>(createOut.MonitorObject));
            m_monitorHandles[connectorIndex] = nullptr;
        }
        else
        {
            USBLOG_INFO(L"CreateMonitor: MonitorArrival succeeded (connector=%u) -- monitor should now enumerate", connectorIndex);
        }
        return status;
    }

    void Device::RemoveAllMonitors()
    {
        for (auto& monitor : m_monitorHandles)
        {
            if (monitor)
            {
                USBLOG_INFO(L"RemoveAllMonitors: departing monitor %p", monitor);
                IddCxMonitorDeparture(monitor);
                WdfObjectDelete(reinterpret_cast<WDFOBJECT>(monitor));
                monitor = nullptr;
            }
        }
    }

    IDDCX_ADAPTER Device::Adapter() const
    {
        return m_adapter;
    }
}
