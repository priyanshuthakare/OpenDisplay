#include "SwapChainProcessor.h"

#include <avrt.h>
#include <utility>

using Microsoft::WRL::ComPtr;

namespace UsbDisplay
{
    Direct3DDevice::Direct3DDevice(LUID adapterLuid) : AdapterLuid(adapterLuid)
    {
    }

    HRESULT Direct3DDevice::Initialize()
    {
        HRESULT hr = CreateDXGIFactory2(0, IID_PPV_ARGS(&Factory));
        if (FAILED(hr))
        {
            return hr;
        }

        hr = Factory->EnumAdapterByLuid(AdapterLuid, IID_PPV_ARGS(&Adapter));
        if (FAILED(hr))
        {
            return hr;
        }

        return D3D11CreateDevice(
            Adapter.Get(),
            D3D_DRIVER_TYPE_UNKNOWN,
            nullptr,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr,
            0,
            D3D11_SDK_VERSION,
            &Device,
            nullptr,
            &Context);
    }

    SwapChainProcessor::SwapChainProcessor(IDDCX_SWAPCHAIN swapChain, std::shared_ptr<Direct3DDevice> device, HANDLE newFrameEvent)
        : m_swapChain(swapChain), m_device(std::move(device)), m_newFrameEvent(newFrameEvent)
    {
        m_stopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (!m_stopEvent)
        {
            WdfObjectDelete(reinterpret_cast<WDFOBJECT>(m_swapChain));
            m_swapChain = nullptr;
            return;
        }

        m_running = true;
        m_thread = CreateThread(nullptr, 0, ThreadEntry, this, 0, nullptr);
        if (!m_thread)
        {
            m_running = false;
            WdfObjectDelete(reinterpret_cast<WDFOBJECT>(m_swapChain));
            m_swapChain = nullptr;
        }
    }

    SwapChainProcessor::~SwapChainProcessor()
    {
        m_running = false;
        if (m_stopEvent)
        {
            SetEvent(m_stopEvent);
        }

        if (m_thread)
        {
            WaitForSingleObject(m_thread, INFINITE);
            CloseHandle(m_thread);
            m_thread = nullptr;
        }

        if (m_stopEvent)
        {
            CloseHandle(m_stopEvent);
            m_stopEvent = nullptr;
        }
    }

    DWORD CALLBACK SwapChainProcessor::ThreadEntry(void* context)
    {
        reinterpret_cast<SwapChainProcessor*>(context)->Run();
        return 0;
    }

    void SwapChainProcessor::Run()
    {
        DWORD taskIndex = 0;
        HANDLE mmcssHandle = AvSetMmThreadCharacteristicsW(L"Distribution", &taskIndex);
        ProcessFrames();

        if (m_swapChain)
        {
            WdfObjectDelete(reinterpret_cast<WDFOBJECT>(m_swapChain));
            m_swapChain = nullptr;
        }

        if (mmcssHandle)
        {
            AvRevertMmThreadCharacteristics(mmcssHandle);
        }
    }

    void SwapChainProcessor::ProcessFrames()
    {
        ComPtr<IDXGIDevice> dxgiDevice;
        HRESULT hr = m_device->Device.As(&dxgiDevice);
        if (FAILED(hr))
        {
            return;
        }

        IDARG_IN_SWAPCHAINSETDEVICE setDevice = {};
        setDevice.pDevice = dxgiDevice.Get();
        hr = IddCxSwapChainSetDevice(m_swapChain, &setDevice);
        if (FAILED(hr))
        {
            return;
        }

        HANDLE waitHandles[] = {m_newFrameEvent, m_stopEvent};
        while (m_running)
        {
            IDARG_OUT_RELEASEANDACQUIREBUFFER buffer = {};
            hr = IddCxSwapChainReleaseAndAcquireBuffer(m_swapChain, &buffer);
            if (hr == E_PENDING)
            {
                const DWORD wait = WaitForMultipleObjects(ARRAYSIZE(waitHandles), waitHandles, FALSE, 16);
                if (wait == WAIT_OBJECT_0 || wait == WAIT_TIMEOUT)
                {
                    continue;
                }
                break;
            }

            if (FAILED(hr))
            {
                break;
            }

            ComPtr<IDXGIResource> surface;
            surface.Attach(buffer.MetaData.pSurface);

            surface.Reset();
            hr = IddCxSwapChainFinishedProcessingFrame(m_swapChain);
            if (FAILED(hr))
            {
                break;
            }
        }
    }
}
