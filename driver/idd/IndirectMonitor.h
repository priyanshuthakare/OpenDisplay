#pragma once

#include <memory>
#include <windows.h>
#include <wdf.h>
#include <iddcx.h>
#include "SwapChainProcessor.h"

namespace UsbDisplay
{
    class IndirectMonitor
    {
    public:
        explicit IndirectMonitor(IDDCX_MONITOR monitor);
        ~IndirectMonitor();

        void AssignSwapChain(IDDCX_SWAPCHAIN swapChain, LUID renderAdapter, HANDLE newFrameEvent);
        void UnassignSwapChain();

    private:
        IDDCX_MONITOR m_monitor = nullptr;
        std::unique_ptr<SwapChainProcessor> m_processor;
    };
}

