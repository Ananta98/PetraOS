import QtQuick
import qs.Commons

Rectangle {
    id: root

    property string text: ""
    property string icon: ""
    property bool active: false
    property bool hovered: mouseArea.containsMouse

    signal clicked()

    implicitWidth: contentRow.implicitWidth + 24
    implicitHeight: 32
    radius: Style.radiusPill
    color: active ? Color.accentSoft : (hovered ? Color.withAlpha(Color.textPrimary, 0.08) : "transparent")
    border.width: active ? 1 : 0
    border.color: active ? Color.accent : "transparent"

    Behavior on color { ColorAnimation { duration: Style.animFast } }
    Behavior on scale { NumberAnimation { duration: Style.animFast } }

    scale: mouseArea.pressed ? 0.96 : 1.0

    Row {
        id: contentRow
        anchors.centerIn: parent
        spacing: 6

        Text {
            visible: root.icon !== ""
            text: root.icon
            font.family: Style.fontFamily
            font.pixelSize: Style.fontSizeBody
            color: active ? Color.accent : Color.textPrimary
            anchors.verticalCenter: parent.verticalCenter
        }

        Text {
            visible: root.text !== ""
            text: root.text
            font.family: Style.fontFamily
            font.pixelSize: Style.fontSizeBody
            font.weight: active ? Font.DemiBold : Font.Normal
            color: active ? Color.accent : Color.textPrimary
            anchors.verticalCenter: parent.verticalCenter
        }
    }

    MouseArea {
        id: mouseArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
