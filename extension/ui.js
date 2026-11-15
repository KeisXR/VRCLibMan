// 拡張の画面で使う小さな部品。文字は textContent で入れ、HTML として解釈しない。
(function (root) {
  const SVG = "http://www.w3.org/2000/svg";

  function h(tag, props, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props || {})) {
      if (value === undefined || value === null || value === false) continue;
      if (key === "class") node.className = value;
      else if (key === "text") node.textContent = value;
      else if (key === "on") for (const [event, handler] of Object.entries(value)) node.addEventListener(event, handler);
      else if (key in node && !key.startsWith("aria-") && key !== "role") node[key] = value;
      else node.setAttribute(key, value === true ? "" : String(value));
    }
    for (const child of children.flat()) {
      if (child === undefined || child === null || child === false) continue;
      node.append(child instanceof Node ? child : document.createTextNode(String(child)));
    }
    return node;
  }

  function chip(text, tone) {
    return h("span", { class: tone ? `chip ${tone}` : "chip", text });
  }

  function badge(text) {
    return h("span", { class: "badge", text });
  }

  // 件数が分かる仕事は件数の帯、分からない仕事は往復する帯。
  function track(current, total, label) {
    const known = Number(total) > 0;
    const percent = known ? Math.max(0, Math.min(100, Math.round((Number(current) / Number(total)) * 100))) : 0;
    const bar = h("div", {
      class: known ? "track" : "track indeterminate",
      role: "progressbar",
      "aria-label": label || "進み具合",
      "aria-valuemin": known ? "0" : undefined,
      "aria-valuemax": known ? "100" : undefined,
      "aria-valuenow": known ? String(percent) : undefined,
    });
    const fill = h("span");
    if (known) fill.style.width = `${percent}%`;
    bar.append(fill);
    return bar;
  }

  const ICONS = {
    panel: [["rect", { x: 3, y: 4, width: 18, height: 16, rx: 3 }], ["line", { x1: 15, y1: 4, x2: 15, y2: 20 }]],
    settings: [
      ["line", { x1: 4, y1: 7, x2: 6.5, y2: 7 }], ["line", { x1: 11.5, y1: 7, x2: 20, y2: 7 }], ["circle", { cx: 9, cy: 7, r: 2.5 }],
      ["line", { x1: 4, y1: 17, x2: 12.5, y2: 17 }], ["line", { x1: 17.5, y1: 17, x2: 20, y2: 17 }], ["circle", { cx: 15, cy: 17, r: 2.5 }],
    ],
    refresh: [["path", { d: "M19.5 12a7.5 7.5 0 1 1-2.2-5.3" }], ["path", { d: "M19.5 4.5v4h-4" }]],
  };

  function icon(name) {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "1.8");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
    svg.setAttribute("aria-hidden", "true");
    for (const [tag, attrs] of ICONS[name] || []) {
      const part = document.createElementNS(SVG, tag);
      for (const [key, value] of Object.entries(attrs)) part.setAttribute(key, String(value));
      svg.append(part);
    }
    return svg;
  }

  // アプリが返した状態（所持・保存記録・対応アバター）を印の並びにする。
  function statusMarks(row, companion) {
    const marks = companion.chips(row).map((mark) => chip(mark.text, mark.tone));
    for (const name of companion.avatarNames(row)) marks.push(badge(name));
    return marks;
  }

  function jobTone(state) {
    return state === "done" ? "good" : state === "running" ? "" : state === "cancelled" ? "" : "warn";
  }

  root.VrcLibManUi = { h, chip, badge, track, icon, statusMarks, jobTone };
})(globalThis);
