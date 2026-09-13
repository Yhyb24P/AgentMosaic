// Progressive enhancement only.
//
// The page is complete and fully usable without this script: every command is real
// selectable text in the DOM, and the copy buttons stay hidden until scripting and the
// async clipboard are both confirmed available. No framework, no toast, no state.
(function () {
  "use strict";

  if (!navigator.clipboard || typeof navigator.clipboard.writeText !== "function") {
    return;
  }

  // Copy the command block verbatim, dropping authoring indentation and blank edges.
  function commandText(element) {
    var lines = element.textContent.replace(/\r\n?/g, "\n").split("\n");
    while (lines.length && lines[0].trim() === "") lines.shift();
    while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();
    if (!lines.length) return "";

    var indent = Infinity;
    lines.forEach(function (line) {
      if (line.trim() === "") return;
      var width = line.length - line.replace(/^[ \t]+/, "").length;
      if (width < indent) indent = width;
    });
    if (indent === Infinity) indent = 0;

    return lines.map(function (line) {
      return line.slice(indent);
    }).join("\n") + "\n";
  }

  function panelFor(button) {
    var node = button.parentNode;
    while (node && node.nodeType === 1) {
      if (node.classList && node.classList.contains("code-panel")) return node;
      node = node.parentNode;
    }
    return null;
  }

  var buttons = document.querySelectorAll("button[data-copy]");
  Array.prototype.forEach.call(buttons, function (button) {
    var source = document.getElementById(button.getAttribute("data-copy"));
    var panel = panelFor(button);
    var status = panel ? panel.querySelector(".copy-status") : null;
    if (!source) return;

    var timer = 0;
    function report(message) {
      if (!status || !message) return;
      window.clearTimeout(timer);
      status.textContent = message;
      timer = window.setTimeout(function () {
        status.textContent = "";
      }, 2000);
    }

    button.hidden = false;
    button.addEventListener("click", function () {
      navigator.clipboard.writeText(commandText(source)).then(
        function () {
          report(button.getAttribute("data-copied"));
        },
        function () {
          report(button.getAttribute("data-failed"));
        }
      );
    });
  });
})();
