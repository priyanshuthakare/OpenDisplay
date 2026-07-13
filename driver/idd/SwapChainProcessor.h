#pragma once

#include <atomic>
#include <memory>
#include <windows.h>
#include <wdf.h>
#include <iddcx.h>
#include <d3d11_4.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include "RenderTest.h"

namespace UsbDisplay
{
    struct Direct3DDevice
    {
        explicit Direct3DDevice(LUID adapterLuid);
        HRESULT Initialize();

        LUID AdapterLuid{};
        Microsoft::WRL::ComPtr<IDXGIFactory6> Factory;
        Microsoft::WRL::ComPtr<IDXGIAdapter1> Adapter;
        Microsoft::WRL::ComPtr<ID3D11Device> Device;
        Microsoft::WRL::ComPtr<ID3D11DeviceContext> Context;
    };

    class SwapChainProcessor
    {
    public:
        SwapChainProcessor(IDDCX_SWAPCHAIN swapChain, std::shared_ptr<Direct3DDevice> device, HANDLE newFrameEvent);
        ~SwapChainProcessor();

        SwapChainProcessor(const SwapChainProcessor&) = delete;
        SwapChainProcessor& operator=(const SwapChainProcessor&) = delete;

    private:
        static DWORD CALLBACK ThreadEntry(void* context);
        void Run();
        void ProcessFrames();

        IDDCX_SWAPCHAIN m_swapChain = nullptr;
        std::shared_ptr<Direct3DDevice> m_device;
        HANDLE m_newFrameEvent = nullptr;
        HANDLE m_stopEvent = nullptr;
        HANDLE m_thread = nullptr;
        std::atomic<bool> m_running = false;
        std::unique_ptr<TestPatternRenderer> m_renderer;
    };
}
