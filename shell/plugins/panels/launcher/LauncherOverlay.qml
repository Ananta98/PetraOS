import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Ui

PanelWindow {
    id: root
    required property var host

    visible: Theme.launcherOpen

    implicitWidth: 460
    implicitHeight: 340
    color: "transparent"

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
    WlrLayershell.namespace: "petra-launcher"

    property string searchQuery: ""

    PopupCard {
        anchors.fill: parent

        Column {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 14

            // Search Input Box
            Rectangle {
                width: parent.width
                height: 40
                radius: Style.radiusButton
                color: Color.surfaceAlt
                border.width: 1
                border.color: Color.accent

                TextInput {
                    id: searchField
                    anchors.fill: parent
                    anchors.margins: 10
                    verticalAlignment: TextInput.AlignVCenter
                    font.family: Style.fontFamily
                    font.pixelSize: 14
                    color: Color.textPrimary
                    onTextChanged: root.searchQuery = text
                    Keys.onEscapePressed: Theme.launcherOpen = false
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.left: parent.left
                    anchors.leftMargin: 12
                    text: "Search applications..."
                    font.family: Style.fontFamily
                    font.pixelSize: 14
                    color: Color.textMuted
                    visible: searchField.text === ""
                }
            }

            // App List
            ListView {
                width: parent.width
                height: parent.height - 54
                clip: true
                spacing: 6
                model: host.appLibrary.search(root.searchQuery)

                delegate: Button {
                    required property var modelData
                    width: ListView.view.width
                    height: 42
                    text: modelData.name + "  (" + modelData.category + ")"
                    icon: modelData.icon
                    onClicked: {
                        host.appLibrary.launch(modelData.exec)
                        Theme.launcherOpen = false
                    }
                }
            }
        }
    }
}
