param(
    [Parameter(Mandatory=$true)][string]$Dll,
    [Parameter(Mandatory=$true)][string]$Rva   # e.g. 0x17cf
)

$ErrorActionPreference = "Stop"
$dllPath = (Resolve-Path $Dll).Path
$rvaVal = [Convert]::ToUInt64($Rva, 16)

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class SymResolver
{
    [DllImport("dbghelp.dll", SetLastError=true)]
    public static extern bool SymInitialize(IntPtr hProcess, string UserSearchPath, bool fInvadeProcess);
    [DllImport("dbghelp.dll", SetLastError=true)]
    public static extern uint SymSetOptions(uint SymOptions);
    [DllImport("dbghelp.dll", SetLastError=true, CharSet=CharSet.Ansi)]
    public static extern ulong SymLoadModuleEx(IntPtr hProcess, IntPtr hFile, string ImageName, string ModuleName, ulong BaseOfDll, uint DllSize, IntPtr Data, uint Flags);
    [DllImport("dbghelp.dll", SetLastError=true)]
    public static extern bool SymFromAddr(IntPtr hProcess, ulong Address, out ulong Displacement, IntPtr Symbol);
    [DllImport("dbghelp.dll", SetLastError=true)]
    public static extern bool SymGetLineFromAddr64(IntPtr hProcess, ulong Address, out uint Displacement, IntPtr Line);
    [DllImport("dbghelp.dll", SetLastError=true)]
    public static extern bool SymCleanup(IntPtr hProcess);

    public const uint SYMOPT_UNDNAME = 0x2;
    public const uint SYMOPT_LOAD_LINES = 0x10;
    public const uint SYMOPT_DEFERRED_LOADS = 0x4;

    // Resolve RVA -> "name+disp" and file:line.
    public static string Resolve(string dllPath, ulong rva)
    {
        IntPtr h = new IntPtr(0x1234);
        SymSetOptions(SYMOPT_UNDNAME | SYMOPT_LOAD_LINES | SYMOPT_DEFERRED_LOADS);
        string dir = System.IO.Path.GetDirectoryName(dllPath);
        if (!SymInitialize(h, dir, false))
            return "SymInitialize failed: " + Marshal.GetLastWin32Error();

        ulong baseAddr = 0x180000000UL;
        ulong loaded = SymLoadModuleEx(h, IntPtr.Zero, dllPath, null, baseAddr, 0, IntPtr.Zero, 0);
        if (loaded == 0)
        {
            int err = Marshal.GetLastWin32Error();
            if (err != 0) return "SymLoadModuleEx failed: " + err;
        }
        // Use the base dbghelp actually loaded at (0 -> already loaded at baseAddr).
        ulong effectiveBase = (loaded != 0) ? loaded : baseAddr;
        ulong addr = effectiveBase + rva;
        Console.WriteLine("    [debug] loadedBase=0x" + effectiveBase.ToString("x") + " targetAddr=0x" + addr.ToString("x"));

        // SYMBOL_INFO: fixed part 88 bytes, then Name buffer.
        int nameCap = 1024;
        int size = 88 + nameCap;
        IntPtr buf = Marshal.AllocHGlobal(size);
        try
        {
            for (int i = 0; i < size; i++) Marshal.WriteByte(buf, i, 0);
            Marshal.WriteInt32(buf, 0, 88);          // SizeOfStruct
            Marshal.WriteInt32(buf, 80, (uint)nameCap == 0 ? 0 : nameCap); // MaxNameLen at offset 80

            ulong disp;
            string result;
            if (SymFromAddr(h, addr, out disp, buf))
            {
                uint nameLen = (uint)Marshal.ReadInt32(buf, 76);
                IntPtr namePtr = new IntPtr(buf.ToInt64() + 84);
                string name = Marshal.PtrToStringAnsi(namePtr, (int)nameLen);
                result = name + "+0x" + disp.ToString("x");
            }
            else
            {
                result = "SymFromAddr failed: " + Marshal.GetLastWin32Error();
            }

            // Line info (IMAGEHLP_LINE64: DWORD SizeOfStruct; PVOID Key; DWORD LineNumber; PCHAR FileName; ULONG64 Address)
            IntPtr lineBuf = Marshal.AllocHGlobal(64);
            try
            {
                for (int i = 0; i < 64; i++) Marshal.WriteByte(lineBuf, i, 0);
                Marshal.WriteInt32(lineBuf, 0, 24); // SizeOfStruct for IMAGEHLP_LINE64
                uint ldisp;
                if (SymGetLineFromAddr64(h, addr, out ldisp, lineBuf))
                {
                    uint lineNo = (uint)Marshal.ReadInt32(lineBuf, 16);       // LineNumber offset (8 key + 8? ) -- best effort
                    IntPtr fnPtr = (IntPtr)Marshal.ReadInt64(lineBuf, 24);
                    string file = fnPtr != IntPtr.Zero ? Marshal.PtrToStringAnsi(fnPtr) : "";
                    result += "\n    at " + file + ":" + lineNo + " (+0x" + ldisp.ToString("x") + ")";
                }
            }
            finally { Marshal.FreeHGlobal(lineBuf); }

            return result;
        }
        finally
        {
            Marshal.FreeHGlobal(buf);
            SymCleanup(h);
        }
    }
}
"@

Write-Host ("DLL : {0}" -f $dllPath)
Write-Host ("RVA : 0x{0:x}" -f $rvaVal)
Write-Host ("PDB : {0}" -f ([System.IO.Path]::ChangeExtension($dllPath, ".pdb")))
Write-Host "----"
Write-Host ([SymResolver]::Resolve($dllPath, $rvaVal))
