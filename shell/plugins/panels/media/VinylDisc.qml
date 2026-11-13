import QtQuick
import qs.Commons

Item {
    id: root
    property bool spinning: true

    width: 84
    height: 84

    Rectangle {
        id: disc
        anchors.fill: parent
        radius: width / 2
        color: "#18181B"
        border.width: 2
        border.color: Color.accent

        // Vinyl grooves
        Rectangle {
            anchors.centerIn: parent
            width: parent.width * 0.78
            height: width
            radius: width / 2
            color: "transparent"
            border.width: 1
            border.color: "#33FFFFFF"
        }

        // Album art label center
        Rectangle {
            anchors.centerIn: parent
            width: 34
            height: 34
            radius: 17
            gradient: Gradient {
                GradientStop { position: 0.0; color: Color.accent }
                GradientStop { position: 1.0; color: Color.accentSecondary }
            }

            // Center spindle hole
            Rectangle {
                anchors.centerIn: parent
                width: 8
                height: 8
                radius: 4
                color: "#18181B"
            }
        }

        // Highlight reflection wedge for rotation visibility
        Rectangle {
            width: 6
            height: 16
            radius: 3
            color: "#60FFFFFF"
            anchors.top: parent.top
            anchors.topMargin: 6
            anchors.horizontalCenter: parent.horizontalCenter
        }

        RotationAnimator on rotation {
            from: 0
            to: 360
            duration: 6000
            loops: Animation.Infinite
            running: root.spinning
        }
    }
}
