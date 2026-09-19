using System;
using System.Collections.Generic;
using System.Linq;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Shapes;
using System.Windows.Threading;
using USBDisplay.ControlApp.Models;
using USBDisplay.ControlApp.ViewModels;

namespace USBDisplay.ControlApp.UI.Views;

/// <summary>
/// Visual pipeline: nodes illuminate sequentially while starting, animate a
/// travelling packet while active, focus the failed node on error.
/// </summary>
public partial class SignalMonitor : UserControl
{
    public static readonly DependencyProperty NodesProperty =
        DependencyProperty.Register(nameof(Nodes), typeof(IEnumerable<PipelineNode>),
            typeof(SignalMonitor), new PropertyMetadata(null, (d, _) => ((SignalMonitor)d).Render()));

    public static readonly DependencyProperty IsActiveProperty =
        DependencyProperty.Register(nameof(IsActive), typeof(bool),
            typeof(SignalMonitor), new PropertyMetadata(false));

    private readonly DispatcherTimer _timer;
    private double _packetT;

    public SignalMonitor()
    {
        InitializeComponent();
        _timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(50) };
        _timer.Tick += (_, _) => { _packetT += 0.02; if (_packetT > 1) _packetT = 0; RenderPacket(); };
        _timer.Start();
        SizeChanged += (_, _) => Render();
    }

    public IEnumerable<PipelineNode>? Nodes
    {
        get => (IEnumerable<PipelineNode>?)GetValue(NodesProperty);
        set => SetValue(NodesProperty, value);
    }

    public bool IsActive
    {
        get => (bool)GetValue(IsActiveProperty);
        set => SetValue(IsActiveProperty, value);
    }

    /// <summary>Raised when a pipeline node is clicked — host navigates to its diagnostics.</summary>
    public event EventHandler<string>? NodeClicked;

    private Ellipse? _packet;

    private void Render()
    {
        Stage.Children.Clear();
        _packet = null;
        var nodes = (Nodes ?? Enumerable.Empty<PipelineNode>()).ToArray();
        if (nodes.Length == 0 || ActualWidth < 50)
        {
            return;
        }
        var accent = (System.Windows.Media.Brush)FindResource("Accent");
        var dim = (System.Windows.Media.Brush)FindResource("Dim");
        var ok = (System.Windows.Media.Brush)FindResource("Ok");
        var warn = (System.Windows.Media.Brush)FindResource("Warn");
        var err = (System.Windows.Media.Brush)FindResource("Err");
        var fg = (System.Windows.Media.Brush)FindResource("Fg");

        var y = 30.0;
        var stepX = ActualWidth / nodes.Length;
        // Backbone
        var line = new Line
        {
            X1 = stepX / 2, X2 = ActualWidth - stepX / 2, Y1 = y, Y2 = y,
            Stroke = IsActive ? accent : dim, StrokeThickness = 2, Opacity = IsActive ? 1 : 0.35,
        };
        Stage.Children.Add(line);

        for (var i = 0; i < nodes.Length; i++)
        {
            var cx = stepX * i + stepX / 2;
            var brush = nodes[i].State switch
            {
                ComponentState.Running or ComponentState.Ready => IsActive ? accent : ok,
                ComponentState.Connecting => accent,
                ComponentState.Warning => warn,
                ComponentState.Failed => err,
                _ => dim,
            };
            var dot = new Ellipse
            {
                Width = 16, Height = 16, Fill = brush,
                Opacity = nodes[i].State == ComponentState.Stopped || nodes[i].State == ComponentState.Unknown ? 0.35 : 1,
                Cursor = System.Windows.Input.Cursors.Hand,
                ToolTip = $"{nodes[i].Name}: {nodes[i].Detail} — click for diagnostics",
            };
            var nodeName = nodes[i].Name;
            dot.MouseLeftButtonUp += (_, _) => NodeClicked?.Invoke(this, nodeName);
            Canvas.SetLeft(dot, cx - 8);
            Canvas.SetTop(dot, y - 8);
            Stage.Children.Add(dot);

            var label = new TextBlock
            {
                Text = nodes[i].Name, Foreground = fg, FontSize = 10,
                TextAlignment = TextAlignment.Center, Width = stepX - 6,
                TextWrapping = TextWrapping.Wrap,
            };
            Canvas.SetLeft(label, cx - (stepX - 6) / 2);
            Canvas.SetTop(label, y + 14);
            Stage.Children.Add(label);

            var detail = new TextBlock
            {
                Text = nodes[i].Detail, Foreground = dim, FontSize = 10,
                TextAlignment = TextAlignment.Center, Width = stepX - 6,
            };
            Canvas.SetLeft(detail, cx - (stepX - 6) / 2);
            Canvas.SetTop(detail, y + 44);
            Stage.Children.Add(detail);
        }
        RenderPacket();
    }

    private void RenderPacket()
    {
        if (_packet != null)
        {
            Stage.Children.Remove(_packet);
            _packet = null;
        }
        var nodes = (Nodes ?? Enumerable.Empty<PipelineNode>()).ToArray();
        if (!IsActive || nodes.Length == 0 || ActualWidth < 50)
        {
            return;
        }
        var stepX = ActualWidth / nodes.Length;
        var x = stepX / 2 + _packetT * (ActualWidth - stepX);
        _packet = new Ellipse { Width = 8, Height = 8, Fill = (System.Windows.Media.Brush)FindResource("Fg") };
        Canvas.SetLeft(_packet, x - 4);
        Canvas.SetTop(_packet, 26);
        Stage.Children.Add(_packet);
    }
}
