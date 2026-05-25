import { H as escape_html, V as attr, n as attr_class, r as bind_props, u as stringify } from "./dev.js";
//#region src/lib/LaneCard.svelte
function LaneCard($$renderer, $$props) {
	$$renderer.component(($$renderer) => {
		let lane = $$props["lane"];
		$$renderer.push(`<article${attr_class(`lane-card ${stringify(lane.posture)}`)}><div class="lane-card-head"><div><span class="mini-label">${escape_html(lane.posture)}</span> <h3>${escape_html(lane.name)}</h3></div> <a class="btn btn-ghost"${attr("href", lane.href)}>View lane</a></div> <div class="lane-metrics"${attr("aria-label", `${lane.name} worker summary`)}><div><strong>${escape_html(lane.active)}</strong> <span>active</span></div> <div><strong>${escape_html(lane.retrying)}</strong> <span>retrying</span></div> <div><strong>${escape_html(lane.blocked)}</strong> <span>blocked</span></div></div> <p>${escape_html(lane.latest)}</p></article>`);
		bind_props($$props, { lane });
	});
}
//#endregion
export { LaneCard as t };
