using System.IO;
using System.Windows.Media.Imaging;
using QRCoder;

namespace USBDisplay.ControlApp.Services;

/// <summary>Renders pairing payloads as QR bitmaps (PC shows, tablet scans).</summary>
public static class QrCodeService
{
    /// <summary>Renders <paramref name="payload"/> as PNG bytes (default 20 px/module).</summary>
    public static byte[] RenderPng(string payload, int pixelsPerModule = 20)
    {
        using var generator = new QRCodeGenerator();
        using var data = generator.CreateQrCode(payload, QRCodeGenerator.ECCLevel.M);
        using var png = new PngByteQRCode(data);
        return png.GetGraphic(pixelsPerModule);
    }

    public static BitmapImage ToBitmapImage(byte[] pngBytes)
    {
        using var stream = new MemoryStream(pngBytes);
        var image = new BitmapImage();
        image.BeginInit();
        image.CacheOption = BitmapCacheOption.OnLoad;
        image.StreamSource = stream;
        image.EndInit();
        image.Freeze();
        return image;
    }
}
