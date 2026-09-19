using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace USBDisplay.ControlApp.Native;

/// <summary>
/// Minimal SetupAPI + CfgMgr32 P/Invoke to query PnP device state without
/// spawning PowerShell: enumerate devices, match hardware IDs, read service,
/// problem code, and live status flags.
/// </summary>
internal static class SetupApi
{
    private const uint DIGCF_PRESENT = 0x00000002;
    private const uint DIGCF_ALLCLASSES = 0x00000004;
    private const uint SPDRP_HARDWAREID = 0x00000001;
    private const uint SPDRP_SERVICE = 0x00000004;
    private const uint SPDRP_FRIENDLYNAME = 0x0000000C;

    internal static readonly IntPtr INVALID_HANDLE = new(-1);

    [StructLayout(LayoutKind.Sequential)]
    internal struct SP_DEVINFO_DATA
    {
        public uint cbSize;
        public Guid ClassGuid;
        public uint DevInst;
        public IntPtr Reserved;
    }

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern IntPtr SetupDiGetClassDevsW(
        ref Guid classGuid, string? enumerator, IntPtr hwndParent, uint flags);

    [DllImport("setupapi.dll", SetLastError = true)]
    internal static extern bool SetupDiEnumDeviceInfo(IntPtr deviceInfoSet, uint memberIndex, ref SP_DEVINFO_DATA deviceInfoData);

    [DllImport("setupapi.dll", SetLastError = true)]
    internal static extern bool SetupDiDestroyDeviceInfoList(IntPtr deviceInfoSet);

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern bool SetupDiGetDeviceInstanceIdW(
        IntPtr deviceInfoSet, ref SP_DEVINFO_DATA deviceInfoData,
        StringBuilder deviceInstanceId, uint deviceInstanceIdSize, out uint requiredSize);

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern bool SetupDiGetDeviceRegistryPropertyW(
        IntPtr deviceInfoSet, ref SP_DEVINFO_DATA deviceInfoData, uint property,
        out uint propertyRegDataType, byte[]? propertyBuffer, uint propertyBufferSize, out uint requiredSize);

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern bool SetupDiGetDevicePropertyW(
        IntPtr deviceInfoSet, ref SP_DEVINFO_DATA deviceInfoData,
        ref DEVPROPKEY propertyKey, out uint propertyType,
        byte[] propertyBuffer, uint propertyBufferSize, out uint requiredSize, uint flags);

    [StructLayout(LayoutKind.Sequential)]
    internal struct DEVPROPKEY
    {
        public Guid fmtid;
        public uint pid;
    }

    // DEVPKEY_Device_HardwareIds / Service / ProblemCode
    internal static readonly DEVPROPKEY DEVPKEY_HardwareIds = new()
    { fmtid = new Guid(0xa45c254e, 0xdf1c, 0x4efd, 0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0), pid = 3 };
    internal static readonly DEVPROPKEY DEVPKEY_Service = new()
    { fmtid = new Guid(0xa45c254e, 0xdf1c, 0x4efd, 0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0), pid = 6 };
    internal static readonly DEVPROPKEY DEVPKEY_ProblemCode = new()
    { fmtid = new Guid(0xa45c254e, 0xdf1c, 0x4efd, 0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0), pid = 30 };

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    internal static extern uint CM_Locate_DevNodeW(out uint devInst, string deviceId, uint flags);

    [DllImport("cfgmgr32.dll")]
    internal static extern uint CM_Get_DevNode_Status(out uint status, out uint problemNumber, uint devInst, uint flags);

    internal const uint CR_SUCCESS = 0;
    internal const uint DN_STARTED = 0x00000008;
    internal const uint DN_HAS_PROBLEM = 0x00000400;
    internal const uint DN_DISABLEABLE = 0x00002000;

    public sealed record PnpDevice(string InstanceId, string HardwareIds, string? Service, string? FriendlyName);

    public static List<PnpDevice> EnumeratePresent()
    {
        var result = new List<PnpDevice>();
        var empty = Guid.Empty;
        var set = SetupDiGetClassDevsW(ref empty, null, IntPtr.Zero, DIGCF_PRESENT | DIGCF_ALLCLASSES);
        if (set == INVALID_HANDLE)
        {
            return result;
        }
        try
        {
            uint index = 0;
            while (true)
            {
                var data = new SP_DEVINFO_DATA { cbSize = (uint)Marshal.SizeOf<SP_DEVINFO_DATA>() };
                if (!SetupDiEnumDeviceInfo(set, index++, ref data))
                {
                    break;
                }
                var sb = new StringBuilder(512);
                if (!SetupDiGetDeviceInstanceIdW(set, ref data, sb, (uint)sb.Capacity, out _))
                {
                    continue;
                }
                var hw = GetMultiStringProperty(set, ref data, DEVPKEY_HardwareIds);
                var svc = GetStringProperty(set, ref data, DEVPKEY_Service);
                var friendly = GetRegistryString(set, ref data, SPDRP_FRIENDLYNAME);
                result.Add(new PnpDevice(sb.ToString(), hw ?? "", svc, friendly));
            }
        }
        finally
        {
            SetupDiDestroyDeviceInfoList(set);
        }
        return result;
    }

    public static (uint Status, uint Problem) GetLiveStatus(string instanceId)
    {
        if (CM_Locate_DevNodeW(out var devInst, instanceId, 0) != CR_SUCCESS)
        {
            return (0, uint.MaxValue);
        }
        if (CM_Get_DevNode_Status(out var status, out var problem, devInst, 0) != CR_SUCCESS)
        {
            return (0, uint.MaxValue);
        }
        return (status, problem);
    }

    private static string? GetMultiStringProperty(IntPtr set, ref SP_DEVINFO_DATA data, DEVPROPKEY key)
    {
        var k = key;
        if (!SetupDiGetDevicePropertyW(set, ref data, ref k, out _, new byte[4096], 4096, out var needed, 0))
        {
            return null;
        }
        // Multi-sz UTF-16 blob; join with ';' like the PowerShell scripts do.
        var buf = new byte[needed > 4096 ? 4096 : needed];
        if (!SetupDiGetDevicePropertyW(set, ref data, ref k, out _, buf, (uint)buf.Length, out _, 0))
        {
            return null;
        }
        var text = Encoding.Unicode.GetString(buf).TrimEnd('\0');
        return text.Replace("\0", ";");
    }

    private static string? GetStringProperty(IntPtr set, ref SP_DEVINFO_DATA data, DEVPROPKEY key)
    {
        var k = key;
        var buf = new byte[1024];
        if (!SetupDiGetDevicePropertyW(set, ref data, ref k, out _, buf, (uint)buf.Length, out var needed, 0))
        {
            return null;
        }
        var text = Encoding.Unicode.GetString(buf, 0, (int)Math.Min(needed, buf.Length));
        return text.TrimEnd('\0');
    }

    private static string? GetRegistryString(IntPtr set, ref SP_DEVINFO_DATA data, uint prop)
    {
        var buf = new byte[1024];
        if (!SetupDiGetDeviceRegistryPropertyW(set, ref data, prop, out _, buf, (uint)buf.Length, out var needed))
        {
            return null;
        }
        var text = Encoding.Unicode.GetString(buf, 0, (int)Math.Min(needed, buf.Length));
        return text.TrimEnd('\0');
    }
}
