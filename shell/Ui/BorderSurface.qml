import QtQuick
import qs.Commons

Rectangle {
    id: root

    property bool elevated: false
    property int cornerRadius: Style.radiusPanel

    color: elevated ? Color.surfaceElevated : Color.surface
    radius: cornerRadius
    border.width: 1
    border.color: Color.border

    Behavior on color {
        ColorAnimation { duration: Style.animNormal }
    }
    Behavior on border.color {
        ColorAnimation { duration: Style.animNormal }
    }
}
