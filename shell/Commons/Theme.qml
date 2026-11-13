pragma Singleton
import QtQuick

QtObject {
    id: root

    // Available themes: "crimson" (Noctalia style), "cream" (Quickshell showcase), "aurora" (Petra signature)
    property string currentTheme: "crimson"
    property string activeTab: "media" // "dashboard", "media", "performance", "workspaces"
    property bool mediaPopupOpen: false
    property bool launcherOpen: false

    function cycleTheme() {
        if (currentTheme === "crimson") currentTheme = "cream"
        else if (currentTheme === "cream") currentTheme = "aurora"
        else currentTheme = "crimson"
    }

    function toggleMediaPopup() {
        mediaPopupOpen = !mediaPopupOpen
    }

    function toggleLauncher() {
        launcherOpen = !launcherOpen
    }
}
