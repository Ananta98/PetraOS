import QtQuick
import Quickshell.Hyprland
import qs.Commons

Row {
    id: root
    spacing: 6

    Repeater {
        model: 5

        Rectangle {
            required property int index
            readonly property int wsId: index + 1
            readonly property bool isActive: Hyprland.focusedWorkspace ? (Hyprland.focusedWorkspace.id === wsId) : (wsId === 1)

            width: isActive ? 22 : 8
            height: 8
            radius: 4
            color: isActive ? Color.accent : Color.withAlpha(Color.textPrimary, 0.25)

            Behavior on width { NumberAnimation { duration: Style.animFast; easing.type: Easing.OutCubic } }
            Behavior on color { ColorAnimation { duration: Style.animFast } }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: IpcRegistry.switchWorkspace(wsId)
            }
        }
    }
}
