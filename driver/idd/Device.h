#pragma once

#include <array>
#include <memory>
#include <vector>
#include <windows.h>
#include <wdf.h>
#include <iddcx.h>
#include "Edid.h"
#include "IndirectMonitor.h"

namespace UsbDisplay
{
    constexpr uint32_t MaxVirtualMonitors = 4;

    class Device;
    class IndirectMonitor;

    struct DeviceContext
    {
        Device* Instance = nullptr;
    };

    struct IndirectMonitorContext
    {
        IndirectMonitor* Monitor = nullptr;
    };

    WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(DeviceContext, WdfObjectGet_DeviceContext);
    WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(IndirectMonitorContext, WdfObjectGet_IndirectMonitorContext);

    class Device
    {
    public:
        explicit Device(WDFDEVICE device);
        ~Device();

        NTSTATUS InitializeAdapter();
        NTSTATUS AdapterInitFinished(NTSTATUS initStatus);
        NTSTATUS CommitModes(const IDARG_IN_COMMITMODES* args);
        NTSTATUS CreateMonitor(uint32_t connectorIndex);
        void RemoveAllMonitors();

        IDDCX_ADAPTER Adapter() const;

    private:
        WDFDEVICE m_wdfDevice = nullptr;
        IDDCX_ADAPTER m_adapter = nullptr;
        bool m_adapterInitStarted = false;
        std::array<IDDCX_MONITOR, MaxVirtualMonitors> m_monitorHandles{};
    };
}
