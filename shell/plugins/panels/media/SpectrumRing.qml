import QtQuick
import qs.Commons

Canvas {
    id: root
    property bool active: true
    property real phase: 0.0

    width: 150
    height: 150

    Timer {
        interval: 40
        running: root.active && root.visible
        repeat: true
        onTriggered: {
            root.phase += 0.14
            root.requestPaint()
        }
    }

    onPaint: {
        var ctx = getContext("2d")
        ctx.reset()
        var cx = width / 2
        var cy = height / 2
        var baseRadius = 48
        var bars = 40

        ctx.strokeStyle = Color.accent.toString()
        ctx.lineWidth = 3
        ctx.lineCap = "round"

        for (var i = 0; i < bars; i++) {
            var angle = (i / bars) * Math.PI * 2
            var amp = root.active
                ? (4 + 10 * Math.abs(Math.sin(phase + i * 0.45) * Math.cos(phase * 0.6 - i * 0.2)))
                : 3
            var x1 = cx + Math.cos(angle) * baseRadius
            var y1 = cy + Math.sin(angle) * baseRadius
            var x2 = cx + Math.cos(angle) * (baseRadius + amp)
            var y2 = cy + Math.sin(angle) * (baseRadius + amp)

            ctx.beginPath()
            ctx.moveTo(x1, y1)
            ctx.lineTo(x2, y2)
            ctx.stroke()
        }
    }
}
