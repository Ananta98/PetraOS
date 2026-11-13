import QtQuick
import qs.Commons

Item {
    id: root

    property real value: 0.5
    signal moved(real newValue)

    implicitWidth: 180
    implicitHeight: 16

    Rectangle {
        id: track
        anchors.verticalCenter: parent.verticalCenter
        width: parent.width
        height: 6
        radius: 3
        color: Color.withAlpha(Color.textPrimary, 0.15)

        Rectangle {
            width: Math.max(6, Math.min(parent.width, parent.width * root.value))
            height: parent.height
            radius: 3
            color: Color.accent
        }
    }

    Rectangle {
        x: Math.max(0, Math.min(parent.width - width, parent.width * root.value - width / 2))
        anchors.verticalCenter: parent.verticalCenter
        width: 14
        height: 14
        radius: 7
        color: "#FFFFFF"
        border.width: 2
        border.color: Color.accent
    }

    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        function updateVal(mx) {
            var v = Math.max(0.0, Math.min(1.0, mx / width))
            root.value = v
            root.moved(v)
        }
        onPressed: (mouse) => updateVal(mouse.x)
        onPositionChanged: (mouse) => { if (pressed) updateVal(mouse.x) }
    }
}
