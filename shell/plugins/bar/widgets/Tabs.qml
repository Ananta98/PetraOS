import QtQuick
import qs.Commons
import qs.Ui

Row {
    id: root
    required property var host
    spacing: 4

    Repeater {
        model: host.barWidgetRegistry.tabs

        Button {
            required property var modelData
            text: modelData.label
            icon: modelData.glyph
            active: Theme.activeTab === modelData.id && Theme.mediaPopupOpen
            onClicked: {
                if (Theme.activeTab === modelData.id) {
                    Theme.toggleMediaPopup()
                } else {
                    Theme.activeTab = modelData.id
                    Theme.mediaPopupOpen = true
                }
            }
        }
    }
}
