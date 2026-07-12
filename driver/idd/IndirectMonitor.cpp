#include "IndirectMonitor.h"

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
        if (FAILED(device->Initialize()))
        {
            WdfObjectDelete(reinterpret_cast<WDFOBJECT>(swapChain));
            return;
        }

        m_processor = std::make_unique<SwapChainProcessor>(swapChain, device, newFrameEvent);
    }

    void IndirectMonitor::UnassignSwapChain()
    {
        m_processor.reset();
    }
}

