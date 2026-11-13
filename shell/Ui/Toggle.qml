import QtQuick
import qs.Commons

Rectangle {
    id: root
    property bool checked: false
    signal toggled(bool state)

    implicitWidth: 44
    implicitHeight: 24
    radius: 12
    color: checked ? Color.accent : Color.withAlpha(Color.textPrimary, 0.2)

    Behavior on color { ColorAnimation { duration: Style.animFast } }

    Rectangle {
        x: root.checked ? parent.width - width - 3 : 3
        anchors.verticalCenter: parent.verticalCenter
        width: 18
        height: 18
        radius: 9
        color: "#FFFFFF"
        Behavior on x { NumberAnimation { duration: Style.animFast; easing.type: Easing.OutCubic } }
    }

    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            root.checked = !root.checked
            root.toggled(root.checked)
        }
    }
}
