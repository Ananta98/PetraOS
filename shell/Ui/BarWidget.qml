import QtQuick
import qs.Commons

Item {
    id: root
    default property alias content: container.data

    implicitWidth: container.implicitWidth
    implicitHeight: container.implicitHeight

    Item {
        id: container
        anchors.fill: parent
        implicitWidth: childrenRect.width
        implicitHeight: childrenRect.height
    }
}
