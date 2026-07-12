#include "Device.h"

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
            m_adapter = adapterInitOut.AdapterObject;
            auto* context = WdfObjectGet_DeviceContext(m_adapter);
            context->Instance = this;
        }
        return status;
    }

    NTSTATUS Device::AdapterInitFinished(NTSTATUS initStatus)
    {
        if (!NT_SUCCESS(initStatus))
        {
            return initStatus;
        }

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
            return status;
        }

        auto* wrapper = WdfObjectGet_IndirectMonitorContext(createOut.MonitorObject);
        wrapper->Monitor = new IndirectMonitor(createOut.MonitorObject);
        m_monitorHandles[connectorIndex] = createOut.MonitorObject;

        IDARG_OUT_MONITORARRIVAL arrival = {};
        status = IddCxMonitorArrival(createOut.MonitorObject, &arrival);
        if (!NT_SUCCESS(status))
        {
            WdfObjectDelete(reinterpret_cast<WDFOBJECT>(createOut.MonitorObject));
            m_monitorHandles[connectorIndex] = nullptr;
        }
        return status;
    }

    void Device::RemoveAllMonitors()
    {
        for (auto& monitor : m_monitorHandles)
        {
            if (monitor)
            {
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
