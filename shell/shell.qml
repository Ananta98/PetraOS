//@ pragma UseQApplication
import QtQuick
import Quickshell
import Quickshell.Hyprland

import qs.Commons
import "services"
import "plugins/background" as BackgroundPlugin
import "plugins/bar" as BarPlugin
import "plugins/services/media" as MediaServicePlugin
import "plugins/panels/media" as MediaPanelPlugin
import "plugins/panels/clock" as ClockPlugin
import "plugins/panels/weather" as WeatherPlugin
import "plugins/panels/monitor" as MonitorPlugin
import "plugins/panels/dock" as DockPlugin
import "plugins/panels/launcher" as LauncherPlugin

ShellRoot {
    id: shell

    // Shared service instances injected into plugins (Omarchy architecture)
    property PluginRegistry pluginRegistry: PluginRegistry {}
    property BarWidgetRegistry barWidgetRegistry: BarWidgetRegistry {}
    property AppLibrary appLibrary: AppLibrary {}
    property BackgroundSwitch backgroundSwitch: BackgroundSwitch {}
    property MediaServicePlugin.Service mediaService: MediaServicePlugin.Service {}

    // Background Wallpaper Engine
    BackgroundPlugin.Background {
        host: shell
    }

    // Top Floating Capsule Island Bar
    BarPlugin.Bar {
        host: shell
    }

    // Tabbed Popup Island (Media Vinyl Visualizer, Bongo Cat, Dashboard, Performance)
    MediaPanelPlugin.Panel {
        host: shell
    }

    // Noctalia-inspired Desktop Ambient Widgets
    WeatherPlugin.WeatherCard {
        host: shell
    }

    MonitorPlugin.SystemCard {
        host: shell
    }

    ClockPlugin.BigClockWidget {
        host: shell
    }

    // Flanking Vertical Docks
    DockPlugin.LeftStatusDock {
        host: shell
    }

    DockPlugin.RightAppDock {
        host: shell
    }

    // Application Launcher Overlay
    LauncherPlugin.LauncherOverlay {
        host: shell
    }
}
