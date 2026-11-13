import QtQuick

QtObject {
    id: root
    property var tabs: [
        { id: "dashboard", label: "Dashboard", glyph: "⊞" },
        { id: "media", label: "Media", glyph: "♫" },
        { id: "performance", label: "Performance", glyph: "◔" },
        { id: "workspaces", label: "Workspaces", glyph: "❖" }
    ]
}
