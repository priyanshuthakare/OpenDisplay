$ErrorActionPreference = "Stop"

$device = Get-PnpDevice -PresentOnly | Where-Object {
    $_.InstanceId -like "ROOT\USBDISPLAYIDD*" -or $_.FriendlyName -like "*USBDisplay*"
} | Select-Object -First 1

if (-not $device) {
    throw "USBDisplay device was not found in Device Manager."
}

Write-Host "Device Manager:"
Write-Host ("  {0} [{1}] {2}" -f $device.FriendlyName, $device.Class, $device.Status)

$display = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorID -ErrorAction SilentlyContinue | Where-Object {
    ($_.UserFriendlyName | ForEach-Object {[char]$_}) -join "" -like "*USBDisplay*"
} | Select-Object -First 1

if ($display) {
    $name = ($display.UserFriendlyName | ForEach-Object {[char]$_}) -join ""
    Write-Host "Windows monitor:"
    Write-Host ("  {0}" -f $name.Trim([char]0))
} else {
    Write-Host "Windows monitor:"
    Write-Host "  WmiMonitorID does not yet show USBDisplay. Open Settings > System > Display or run DisplaySwitch.exe /extend after the driver starts."
}

