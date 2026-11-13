import QtQuick
import qs.Commons
import "AppSearch.js" as AppSearch

QtObject {
    id: root

    property var apps: [
        { name: "Foot Terminal", icon: ">_", exec: "foot", category: "System" },
        { name: "Htop Monitor", icon: "⚡", exec: "foot -e htop", category: "System" },
        { name: "Neovim Editor", icon: "✎", exec: "foot -e vim", category: "Development" },
        { name: "File Browser", icon: "📁", exec: "foot -e sh -c 'ls -la; exec bash'", category: "Utilities" },
        { name: "System Shell", icon: "⌘", exec: "foot -e bash", category: "System" }
    ]

    function search(query) {
        return AppSearch.filterApps(apps, query)
    }

    function launch(execCmd) {
        IpcRegistry.launchCommand(execCmd)
    }
}
