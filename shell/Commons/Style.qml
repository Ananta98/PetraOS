pragma Singleton
import QtQuick

QtObject {
    id: root

    // Radii
    readonly property int radiusPill: 999
    readonly property int radiusCard: 20
    readonly property int radiusPanel: 24
    readonly property int radiusButton: 12
    readonly property int radiusSmall: 8

    // Spacing & Margins
    readonly property int marginScreen: 14
    readonly property int paddingPanel: 16
    readonly property int paddingCard: 14
    readonly property int spacingSmall: 6
    readonly property int spacingMedium: 12
    readonly property int spacingLarge: 18

    // Sizing
    readonly property int topBarHeight: 42
    readonly property int dockWidth: 50
    readonly property int iconSizeSmall: 16
    readonly property int iconSizeMedium: 22
    readonly property int iconSizeLarge: 28

    // Typography
    readonly property string fontFamily: "DejaVu Sans"
    readonly property string fontMono: "DejaVu Sans Mono"
    readonly property int fontSizeSmall: 11
    readonly property int fontSizeBody: 13
    readonly property int fontSizeTitle: 16
    readonly property int fontSizeClockHuge: 72

    // Animation durations (ms)
    readonly property int animFast: 150
    readonly property int animNormal: 260
    readonly property int animSlow: 400
}
