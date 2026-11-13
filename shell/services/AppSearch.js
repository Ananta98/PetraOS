.pragma library

function filterApps(apps, query) {
    if (!query || query.trim() === "") return apps;
    var q = query.toLowerCase();
    var res = [];
    for (var i = 0; i < apps.length; i++) {
        var a = apps[i];
        if (a.name.toLowerCase().indexOf(q) !== -1 ||
            a.category.toLowerCase().indexOf(q) !== -1 ||
            a.exec.toLowerCase().indexOf(q) !== -1) {
            res.push(a);
        }
    }
    return res;
}
