import QtQuick

QtObject {
    id: root
    property int registryRevision: 0
    property var loadedPlugins: [
        "bar", "background", "panels/media", "panels/clock",
        "panels/weather", "panels/monitor", "panels/dock", "panels/launcher"
    ]
}
