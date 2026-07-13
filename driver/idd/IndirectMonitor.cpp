#include "IndirectMonitor.h"
#include "Trace.h"

namespace UsbDisplay
{
    IndirectMonitor::IndirectMonitor(IDDCX_MONITOR monitor) : m_monitor(monitor)
    {
    }

    IndirectMonitor::~IndirectMonitor()
    {
        UnassignSwapChain();
    }

    void IndirectMonitor::AssignSwapChain(IDDCX_SWAPCHAIN swapChain, LUID renderAdapter, HANDLE newFrameEvent)
    {
        UnassignSwapChain();

        auto device = std::make_shared<Direct3DDevice>(renderAdapter);
        HRESULT hr = device->Initialize();
        if (FAILED(hr))
        {
            USBLOG_ERROR(L"AssignSwapChain: Direct3DDevice::Initialize failed 0x%08X; releasing swapchain", hr);
            WdfObjectDelete(reinterpret_cast<WDFOBJECT>(swapChain));
            return;
        }

        USBLOG_INFO(L"AssignSwapChain: D3D device ready; starting swap-chain processor");
        m_processor = std::make_unique<SwapChainProcessor>(swapChain, device, newFrameEvent);
    }

    void IndirectMonitor::UnassignSwapChain()
    {
        if (m_processor)
        {
            USBLOG_INFO(L"UnassignSwapChain: stopping swap-chain processor");
        }
        m_processor.reset();
    }
}

