pragma Singleton
import QtQuick

QtObject {
    id: root

    function formatTime(date) {
        var h = date.getHours()
        var m = date.getMinutes()
        return (h < 10 ? "0" + h : "" + h) + ":" + (m < 10 ? "0" + m : "" + m)
    }

    function formatDateLong(date) {
        var days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]
        var months = ["January", "February", "March", "April", "May", "June",
                      "July", "August", "September", "October", "November", "December"]
        return days[date.getDay()] + ", " + date.getDate() + " " + months[date.getMonth()]
    }

    function formatDuration(seconds) {
        var s = Math.max(0, Math.floor(seconds))
        var mins = Math.floor(s / 60)
        var secs = s % 60
        return mins + ":" + (secs < 10 ? "0" + secs : "" + secs)
    }
}
