import QtQuick
import qs.Commons

Item {
    id: root
    property bool active: true
    property int beatFrame: 0

    width: 110
    height: 84

    Timer {
        interval: 260
        running: root.active && root.visible
        repeat: true
        onTriggered: {
            root.beatFrame = (root.beatFrame + 1) % 3
            catCanvas.requestPaint()
        }
    }

    Canvas {
        id: catCanvas
        anchors.fill: parent
        onPaint: {
            var ctx = getContext("2d")
            ctx.reset()
            ctx.lineWidth = 2.5
            ctx.lineCap = "round"
            ctx.lineJoin = "round"
            ctx.strokeStyle = Color.textPrimary.toString()
            ctx.fillStyle = "#FFFFFF"

            // Table line
            ctx.beginPath()
            ctx.moveTo(8, 70)
            ctx.lineTo(102, 70)
            ctx.stroke()

            // Cat head & ears
            ctx.beginPath()
            ctx.moveTo(28, 66)
            ctx.lineTo(32, 34)
            ctx.lineTo(38, 18) // left ear
            ctx.lineTo(48, 28)
            ctx.lineTo(64, 28)
            ctx.lineTo(74, 18) // right ear
            ctx.lineTo(80, 34)
            ctx.lineTo(84, 66)
            ctx.closePath()
            ctx.fill()
            ctx.stroke()

            // Eyes
            ctx.fillStyle = "#18181B"
            ctx.beginPath()
            ctx.arc(46, 42, 2.5, 0, Math.PI * 2)
            ctx.arc(66, 42, 2.5, 0, Math.PI * 2)
            ctx.fill()

            // Mouth (:3)
            ctx.beginPath()
            ctx.arc(53, 47, 3, 0, Math.PI)
            ctx.arc(59, 47, 3, 0, Math.PI)
            ctx.stroke()

            // Paws (tapping on beatFrame)
            var leftUp = (root.beatFrame === 1)
            var rightUp = (root.beatFrame === 2)

            ctx.fillStyle = "#FFFFFF"
            // Left paw
            ctx.beginPath()
            ctx.arc(36, leftUp ? 54 : 67, 6, 0, Math.PI * 2)
            ctx.fill()
            ctx.stroke()

            // Right paw
            ctx.beginPath()
            ctx.arc(76, rightUp ? 54 : 67, 6, 0, Math.PI * 2)
            ctx.fill()
            ctx.stroke()

            // Accent tap sparks when paw hits table
            if (!leftUp && root.active) {
                ctx.strokeStyle = Color.accent.toString()
                ctx.beginPath()
                ctx.moveTo(26, 74); ctx.lineTo(22, 78)
                ctx.moveTo(32, 75); ctx.lineTo(30, 80)
                ctx.stroke()
            }
            if (!rightUp && root.active) {
                ctx.strokeStyle = Color.accent.toString()
                ctx.beginPath()
                ctx.moveTo(86, 74); ctx.lineTo(90, 78)
                ctx.moveTo(80, 75); ctx.lineTo(82, 80)
                ctx.stroke()
            }
        }
    }
}
