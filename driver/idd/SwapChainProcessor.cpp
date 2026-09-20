#include "SwapChainProcessor.h"
#include "FrameCapture.h"
#include "Trace.h"

#include <avrt.h>
#include <utility>

using Microsoft::WRL::ComPtr;


namespace
{
    bool WriteBmp(const UsbDisplay::FrameCapturer& capture, unsigned long long sequence)
    {
        if (capture.Width() == 0 || capture.Height() == 0 || capture.Pixels().empty())
            return false;

        CreateDirectoryW(L"C:\\ProgramData\\USBDisplay", nullptr);
        CreateDirectoryW(L"C:\\ProgramData\\USBDisplay\\capture", nullptr);

        wchar_t path[256] = {};
        wchar_t temp[256] = {};
        swprintf_s(path, L"C:\\ProgramData\\USBDisplay\\capture\\capture_%016llX.bmp", sequence);
        swprintf_s(temp, L"C:\\ProgramData\\USBDisplay\\capture\\capture_%016llX.tmp", sequence);

        BITMAPFILEHEADER fileHeader = {};
        BITMAPINFOHEADER infoHeader = {};
        infoHeader.biSize = sizeof(infoHeader);
        infoHeader.biWidth = static_cast<LONG>(capture.Width());
        infoHeader.biHeight = -static_cast<LONG>(capture.Height());
        infoHeader.biPlanes = 1;
        infoHeader.biBitCount = 32;
        infoHeader.biCompression = BI_RGB;
        infoHeader.biSizeImage = static_cast<DWORD>(capture.Pixels().size() * sizeof(uint32_t));
        fileHeader.bfType = 0x4D42;
        fileHeader.bfOffBits = sizeof(fileHeader) + sizeof(infoHeader);
        fileHeader.bfSize = fileHeader.bfOffBits + infoHeader.biSizeImage;

        HANDLE file = CreateFileW(temp, GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_ALWAYS,
                                  FILE_ATTRIBUTE_TEMPORARY, nullptr);
        if (file == INVALID_HANDLE_VALUE)
            return false;

        DWORD written = 0;
        bool ok = WriteFile(file, &fileHeader, sizeof(fileHeader), &written, nullptr) &&
                  written == sizeof(fileHeader);
        ok = ok && WriteFile(file, &infoHeader, sizeof(infoHeader), &written, nullptr) &&
             written == sizeof(infoHeader);
        const auto bytes = capture.Pixels().size() * sizeof(uint32_t);
        ok = ok && WriteFile(file, capture.Pixels().data(), static_cast<DWORD>(bytes), &written, nullptr) &&
             written == bytes;
        FlushFileBuffers(file);
        CloseHandle(file);
        if (!ok)
        {
            DeleteFileW(temp);
            return false;
        }
        return MoveFileExW(temp, path, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) != FALSE;
    }
}

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
            USBLOG_ERROR(L"Direct3DDevice: CreateDXGIFactory2 failed 0x%08X", hr);
            return hr;
        }

        hr = Factory->EnumAdapterByLuid(AdapterLuid, IID_PPV_ARGS(&Adapter));
        if (FAILED(hr))
        {
            USBLOG_ERROR(L"Direct3DDevice: EnumAdapterByLuid failed 0x%08X", hr);
            return hr;
        }

        hr = D3D11CreateDevice(
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
        if (FAILED(hr))
        {
            USBLOG_ERROR(L"Direct3DDevice: D3D11CreateDevice failed 0x%08X", hr);
        }
        return hr;
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
            USBLOG_ERROR(L"ProcessFrames: query IDXGIDevice failed 0x%08X", hr);
            return;
        }

        IDARG_IN_SWAPCHAINSETDEVICE setDevice = {};
        setDevice.pDevice = dxgiDevice.Get();
        hr = IddCxSwapChainSetDevice(m_swapChain, &setDevice);
        if (FAILED(hr))
        {
            // 0x887A0026 = DXGI_ERROR_ACCESS_LOST: the OS revoked this swap-chain
            // (common when a fresh assign is immediately superseded). Return quietly;
            // IddCx assigns a new swap-chain and starts us again.
            if (hr == DXGI_ERROR_ACCESS_LOST)
                USBLOG_INFO(L"ProcessFrames: SetDevice access lost (0x%08X); awaiting reassign", hr);
            else
                USBLOG_ERROR(L"ProcessFrames: IddCxSwapChainSetDevice failed 0x%08X", hr);
            return;
        }

        USBLOG_INFO(L"ProcessFrames: entering frame loop");
        FrameCapturer capture(m_device->Device, m_device->Context);
        UINT64 frameCount = 0;
        LARGE_INTEGER freq = {}; QueryPerformanceFrequency(&freq);
        LARGE_INTEGER start = {}; QueryPerformanceCounter(&start);
        LARGE_INTEGER lastFpsT = start;
        UINT64 lastFpsFrame = 0;
        double fps = 0.0;

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
                if (hr == DXGI_ERROR_ACCESS_LOST)
                    USBLOG_INFO(L"ProcessFrames: buffer access lost after %llu frames; awaiting reassign", frameCount);
                else
                    USBLOG_ERROR(L"ProcessFrames: ReleaseAndAcquireBuffer failed 0x%08X after %llu frames", hr, frameCount);
                break;
            }

            ComPtr<IDXGIResource> surface;
            surface.Attach(buffer.MetaData.pSurface);

            LARGE_INTEGER now = {}; QueryPerformanceCounter(&now);

            if (frameCount == 0 || (frameCount % 60) == 0)
            {
                USBLOG_INFO(L"AcquireFrame: frame %llu acquired (fps=%d.%d)",
                            frameCount, (int)fps, (int)(fps * 10) % 10);
            }

            ComPtr<ID3D11Texture2D> texture;
            if (SUCCEEDED(surface.As(&texture)) && capture.Capture(texture.Get()))
            {
                if (!WriteBmp(capture, frameCount))
                    USBLOG_WARN(L"ProcessFrames: failed to publish capture frame %llu", frameCount);
            }

            surface.Reset();
            hr = IddCxSwapChainFinishedProcessingFrame(m_swapChain);
            if (FAILED(hr))
            {
                USBLOG_ERROR(L"ReleaseFrame: FinishedProcessingFrame failed 0x%08X at frame %llu", hr, frameCount);
                break;
            }
            ++frameCount;

            double since = static_cast<double>(now.QuadPart - lastFpsT.QuadPart) / freq.QuadPart;
            if (since >= 1.0)
            {
                fps = (frameCount - lastFpsFrame) / since;
                lastFpsT = now;
                lastFpsFrame = frameCount;
            }
        }
        USBLOG_INFO(L"ProcessFrames: frame loop exited after %llu frames", frameCount);
    }
}
