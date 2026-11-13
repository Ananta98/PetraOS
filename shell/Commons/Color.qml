pragma Singleton
import QtQuick

QtObject {
    id: root

    readonly property bool isCream: Theme.currentTheme === "cream"
    readonly property bool isAurora: Theme.currentTheme === "aurora"
    readonly property bool isCrimson: Theme.currentTheme === "crimson"

    // Surface colors with glassmorphism alpha
    readonly property color surface: isCream ? "#F2F7F2E8" : (isAurora ? "#E60F172A" : "#E6120E10")
    readonly property color surfaceAlt: isCream ? "#E8EFE6D8" : (isAurora ? "#D91E293B" : "#D91E1418")
    readonly property color surfaceElevated: isCream ? "#FAFFF9F0" : (isAurora ? "#F21E293B" : "#F21C1215")

    // Border highlights (1px inner sheen)
    readonly property color border: isCream ? "#408B5E3C" : (isAurora ? "#4538BDF8" : "#40DC2626")
    readonly property color borderSubtle: isCream ? "#208B5E3C" : "#25FFFFFF"

    // Typography colors
    readonly property color textPrimary: isCream ? "#2C221E" : "#F8FAFC"
    readonly property color textSecondary: isCream ? "#6B5B53" : "#94A3B8"
    readonly property color textMuted: isCream ? "#9C8C84" : "#64748B"

    // Signature Accents
    readonly property color accent: isCream ? "#B4533C" : (isAurora ? "#38BDF8" : "#E11D48")
    readonly property color accentSoft: isCream ? "#28B4533C" : (isAurora ? "#2838BDF8" : "#28E11D48")
    readonly property color accentSecondary: isCream ? "#D97757" : (isAurora ? "#A855F7" : "#FB7185")

    // Semantic status
    readonly property color success: "#10B981"
    readonly property color warning: "#F59E0B"
    readonly property color danger: "#EF4444"

    // Wallpaper gradient stops (when no image wallpaper is loaded)
    readonly property color bgTop: isCream ? "#F3EDE2" : (isAurora ? "#090D16" : "#180609")
    readonly property color bgMid: isCream ? "#E8DFD1" : (isAurora ? "#111C33" : "#2B0910")
    readonly property color bgBottom: isCream ? "#D7C9B8" : (isAurora ? "#050811" : "#0A0406")

    function withAlpha(c, a) {
        return Qt.rgba(c.r, c.g, c.b, a)
    }
}
