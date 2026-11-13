import QtQuick

QtObject {
    id: root

    property bool playing: true
    property string title: "Bad Apple!! feat. nomico"
    property string artist: "Alstroemeria Records"
    property string album: "PetraOS Audio Session"
    property string source: "PetraPlayer"
    property real position: 11.0
    property real duration: 317.0
    property real progress: duration > 0 ? (position / duration) : 0.0

    property Timer tickTimer: Timer {
        interval: 1000
        running: root.playing
        repeat: true
        onTriggered: {
            root.position += 1.0
            if (root.position >= root.duration) {
                root.position = 0.0
            }
        }
    }

    function togglePlay() {
        playing = !playing
    }

    function nextTrack() {
        if (title.indexOf("Bad Apple") !== -1) {
            title = "Night of Nights (Petra Mix)"
            artist = "BEAT MARIO"
            position = 0
            duration = 214
        } else {
            title = "Bad Apple!! feat. nomico"
            artist = "Alstroemeria Records"
            position = 0
            duration = 317
        }
    }

    function prevTrack() {
        position = 0
    }

    function seek(ratio) {
        position = Math.max(0, Math.min(duration, ratio * duration))
    }
}
