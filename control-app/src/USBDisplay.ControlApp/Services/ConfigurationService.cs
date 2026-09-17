using System;
using System.IO;
using System.Text.Json;
using USBDisplay.ControlApp.Models;

namespace USBDisplay.ControlApp.Services;

public interface IConfigurationService
{
    AppSettings Settings { get; }
    string SettingsPath { get; }
    void Save();
    void Reload();
}

public sealed class ConfigurationService : IConfigurationService
{
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };

    public AppSettings Settings { get; private set; } = new();
    public string SettingsPath { get; }

    public ConfigurationService()
        : this(Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "USBDisplay", "control-app-settings.json"))
    {
    }

    public ConfigurationService(string settingsPath)
    {
        SettingsPath = settingsPath;
        Reload();
    }

    public void Save()
    {
        var dir = Path.GetDirectoryName(SettingsPath);
        if (!string.IsNullOrEmpty(dir))
        {
            Directory.CreateDirectory(dir);
        }
        File.WriteAllText(SettingsPath, JsonSerializer.Serialize(Settings, JsonOptions));
    }

    public void Reload()
    {
        try
        {
            if (File.Exists(SettingsPath))
            {
                var loaded = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(SettingsPath));
                if (loaded != null)
                {
                    Settings = loaded;
                    return;
                }
            }
        }
        catch
        {
            // Corrupt settings must never brick the app; fall back to defaults.
        }
        Settings = new AppSettings();
    }
}
