pragma Singleton
import QtQuick
import Quickshell.Hyprland

QtObject {
    id: root

    function switchWorkspace(id) {
        Hyprland.dispatch("workspace " + id)
    }

    function launchCommand(cmd) {
        Hyprland.dispatch("exec " + cmd)
    }
}
