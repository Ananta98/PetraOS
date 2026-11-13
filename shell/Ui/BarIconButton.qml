import QtQuick
import qs.Commons

Rectangle {
    id: root

    property string glyph: ""
    property string tooltip: ""
    property bool active: false
    property bool hovered: mouseArea.containsMouse

    signal clicked()

    implicitWidth: 36
    implicitHeight: 36
    radius: Style.radiusButton
    color: active ? Color.accent : (hovered ? Color.accentSoft : "transparent")

    scale: hovered ? 1.12 : (mouseArea.pressed ? 0.94 : 1.0)
    Behavior on scale { NumberAnimation { duration: Style.animFast; easing.type: Easing.OutBack } }
    Behavior on color { ColorAnimation { duration: Style.animFast } }

    Text {
        anchors.centerIn: parent
        text: root.glyph
        font.family: Style.fontFamily
        font.pixelSize: 16
        color: active ? "#FFFFFF" : (hovered ? Color.accent : Color.textPrimary)
    }

    MouseArea {
        id: mouseArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
