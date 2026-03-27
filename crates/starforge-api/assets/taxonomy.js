const state = {
  document: null,
  selectedId: null,
  query: "",
  status: "all",
  domain: "all",
};

const queryInput = document.getElementById("queryInput");
const statusFilter = document.getElementById("statusFilter");
const domainFilter = document.getElementById("domainFilter");
const datasetMeta = document.getElementById("datasetMeta");
const resultsMeta = document.getElementById("resultsMeta");
const entryList = document.getElementById("entryList");
const detailPanel = document.getElementById("detailPanel");

queryInput.addEventListener("input", () => {
  state.query = queryInput.value.trim().toLowerCase();
  render();
});

statusFilter.addEventListener("change", () => {
  state.status = statusFilter.value;
  render();
});

domainFilter.addEventListener("change", () => {
  state.domain = domainFilter.value;
  render();
});

window.addEventListener("hashchange", () => {
  if (!state.document) {
    return;
  }
  const next = currentHashId();
  if (next && state.document.entryMap.has(next)) {
    state.selectedId = next;
    renderDetail();
    renderList();
  }
});

bootstrap();

async function bootstrap() {
  try {
    const response = await fetch("/taxonomy.json", { headers: { accept: "application/json" } });
    if (!response.ok) {
      throw new Error("Request failed with status " + response.status);
    }

    const documentData = await response.json();
    hydrateDocument(documentData);
    state.selectedId = currentHashId() || documentData.root_ids[0] || null;
    renderDomainOptions();
    render();
  } catch (error) {
    datasetMeta.textContent = "Failed to load taxonomy data.";
    resultsMeta.textContent = "0 entries";
    detailPanel.innerHTML =
      '<div class="empty-state"><p>' + escapeHtml(String(error)) + "</p></div>";
  }
}

function hydrateDocument(documentData) {
  const entryMap = new Map();
  documentData.entries.forEach((entry) => {
    entry.depth = depthForEntry(entry, documentData.entries);
    entry.searchText = buildSearchText(entry);
    entryMap.set(entry.id, entry);
  });

  documentData.entryMap = entryMap;
  state.document = documentData;
}

function buildSearchText(entry) {
  const runtimeText = entry.runtime_values
    .map((value) => value.binding + " " + JSON.stringify(value.value))
    .join(" ");
  const sourceText = [
    ...entry.reference_sources.map((source) => source.label),
    ...entry.implementation_sources.map((source) => source.label),
    ...entry.behavior_sources.map((source) => source.label),
  ].join(" ");

  return [
    entry.id,
    entry.title,
    entry.kind,
    entry.status,
    entry.domain_title,
    entry.summary,
    sourceText,
    runtimeText,
  ]
    .join(" ")
    .toLowerCase();
}

function depthForEntry(entry, allEntries) {
  const byId = new Map(allEntries.map((candidate) => [candidate.id, candidate]));
  let depth = 0;
  let parentId = entry.parent_id;
  while (parentId) {
    depth += 1;
    parentId = byId.get(parentId)?.parent_id || null;
  }
  return depth;
}

function renderDomainOptions() {
  const fragment = document.createDocumentFragment();
  const domains = new Map();

  state.document.entries.forEach((entry) => {
    domains.set(entry.domain_id, entry.domain_title);
  });

  Array.from(domains.entries())
    .sort((left, right) => left[1].localeCompare(right[1]))
    .forEach(([id, title]) => {
      const option = document.createElement("option");
      option.value = id;
      option.textContent = title;
      fragment.appendChild(option);
    });

  domainFilter.appendChild(fragment);
}

function render() {
  if (!state.document) {
    return;
  }

  const visibleEntries = filteredEntries();
  const selectedVisible = visibleEntries.some((entry) => entry.id === state.selectedId);
  if (!selectedVisible) {
    state.selectedId = visibleEntries[0]?.id || null;
    syncHash();
  }

  datasetMeta.textContent =
    state.document.ruleset_name +
    " ruleset • " +
    state.document.scenario_name +
    " scenario • " +
    state.document.entries.length +
    " total entries";
  resultsMeta.textContent = visibleEntries.length + " visible entries";
  renderList(visibleEntries);
  renderDetail();
}

function filteredEntries() {
  return state.document.entries.filter((entry) => {
    if (state.status !== "all" && entry.status !== state.status) {
      return false;
    }
    if (state.domain !== "all" && entry.domain_id !== state.domain) {
      return false;
    }
    if (state.query && !entry.searchText.includes(state.query)) {
      return false;
    }
    return true;
  });
}

function renderList(entries = filteredEntries()) {
  entryList.innerHTML = "";

  if (entries.length === 0) {
    entryList.innerHTML = '<div class="empty-state"><p>No entries match the current filters.</p></div>';
    return;
  }

  const fragment = document.createDocumentFragment();
  entries.forEach((entry) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "entry-button" + (entry.id === state.selectedId ? " selected" : "");
    button.style.marginLeft = entry.depth * 12 + "px";
    button.addEventListener("click", () => {
      state.selectedId = entry.id;
      syncHash();
      render();
    });

    button.innerHTML =
      '<div class="entry-title-row">' +
      "<strong>" +
      escapeHtml(entry.title) +
      "</strong>" +
      renderBadge(entry.status) +
      "</div>" +
      '<div class="entry-meta">' +
      '<span class="entry-kind">' +
      escapeHtml(entry.kind.replaceAll("_", " ")) +
      "</span>" +
      "<span>" +
      escapeHtml(entry.id) +
      "</span>" +
      "</div>";
    fragment.appendChild(button);
  });

  entryList.appendChild(fragment);
}

function renderDetail() {
  if (!state.document || !state.selectedId) {
    detailPanel.innerHTML =
      '<div class="empty-state"><p>No taxonomy entry is currently selected.</p></div>';
    return;
  }

  const entry = state.document.entryMap.get(state.selectedId);
  if (!entry) {
    detailPanel.innerHTML =
      '<div class="empty-state"><p>The selected taxonomy entry could not be found.</p></div>';
    return;
  }

  detailPanel.innerHTML =
    '<div class="detail-header">' +
    '<div class="detail-header-top">' +
    "<div>" +
    '<p class="detail-id">' +
    escapeHtml(entry.id) +
    "</p>" +
    "<h2>" +
    escapeHtml(entry.title) +
    "</h2>" +
    '<p class="subtitle">' +
    escapeHtml(entry.summary) +
    "</p>" +
    "</div>" +
    renderBadge(entry.status) +
    "</div>" +
    '<div class="entry-meta">' +
    "<span>Domain: " +
    escapeHtml(entry.domain_title) +
    "</span>" +
    "<span>Kind: " +
    escapeHtml(entry.kind.replaceAll("_", " ")) +
    "</span>" +
    "</div>" +
    "</div>" +
    '<div class="detail-grid">' +
    renderSourceBlock("Reference sections", entry.reference_sources) +
    renderSourceBlock("Implementation links", entry.implementation_sources) +
    renderSourceBlock("Behavior coverage", entry.behavior_sources) +
    renderRuntimeBlock(entry.runtime_values) +
    renderRelatedBlock(entry.related_ids) +
    renderChildBlock(entry.child_ids) +
    "</div>";
}

function renderSourceBlock(title, items) {
  if (!items.length) {
    return renderEmptyMetaBlock(title, "None");
  }

  return (
    '<section class="meta-block">' +
    "<h3>" +
    escapeHtml(title) +
    "</h3>" +
    '<ul class="source-list">' +
    items
      .map(
        (item) =>
          "<li><span>" +
          escapeHtml(item.label) +
          "</span></li>",
      )
      .join("") +
    "</ul>" +
    "</section>"
  );
}

function renderRuntimeBlock(items) {
  if (!items.length) {
    return renderEmptyMetaBlock("Runtime values", "No ruleset or scenario values are bound to this entry.");
  }

  return (
    '<section class="meta-block">' +
    "<h3>Runtime values</h3>" +
    '<ul class="runtime-list">' +
    items
      .map(
        (item) =>
          "<li>" +
          '<span class="runtime-binding">' +
          escapeHtml(item.binding) +
          "</span>" +
          '<span class="runtime-value">' +
          escapeHtml(prettyValue(item.value)) +
          "</span>" +
          "</li>",
      )
      .join("") +
    "</ul>" +
    "</section>"
  );
}

function renderRelatedBlock(relatedIds) {
  if (!relatedIds.length) {
    return renderEmptyMetaBlock("Related entries", "None");
  }

  return (
    '<section class="meta-block">' +
    "<h3>Related entries</h3>" +
    '<ul class="related-list">' +
    relatedIds
      .map((relatedId) => {
        const relatedEntry = state.document.entryMap.get(relatedId);
        if (!relatedEntry) {
          return "";
        }
        return (
          "<li>" +
          '<a href="#' +
          encodeURIComponent(relatedId) +
          '">' +
          escapeHtml(relatedEntry.title) +
          " (" +
          escapeHtml(relatedId) +
          ")</a>" +
          "</li>"
        );
      })
      .join("") +
    "</ul>" +
    "</section>"
  );
}

function renderChildBlock(childIds) {
  if (!childIds.length) {
    return renderEmptyMetaBlock("Child entries", "Leaf entry");
  }

  return (
    '<section class="meta-block">' +
    "<h3>Child entries</h3>" +
    '<ul class="related-list">' +
    childIds
      .map((childId) => {
        const childEntry = state.document.entryMap.get(childId);
        return (
          "<li>" +
          '<a href="#' +
          encodeURIComponent(childId) +
          '">' +
          escapeHtml(childEntry.title) +
          " (" +
          escapeHtml(childId) +
          ")</a>" +
          "</li>"
        );
      })
      .join("") +
    "</ul>" +
    "</section>"
  );
}

function renderEmptyMetaBlock(title, copy) {
  return (
    '<section class="meta-block">' +
    "<h3>" +
    escapeHtml(title) +
    "</h3>" +
    "<p>" +
    escapeHtml(copy) +
    "</p>" +
    "</section>"
  );
}

function renderBadge(status) {
  return '<span class="badge ' + escapeHtml(status) + '">' + escapeHtml(status) + "</span>";
}

function prettyValue(value) {
  if (typeof value === "string") {
    return value;
  }
  return JSON.stringify(value, null, 2);
}

function currentHashId() {
  if (!window.location.hash.startsWith("#")) {
    return null;
  }
  return decodeURIComponent(window.location.hash.slice(1));
}

function syncHash() {
  if (!state.selectedId) {
    return;
  }
  window.location.hash = encodeURIComponent(state.selectedId);
}

function escapeHtml(input) {
  return String(input)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
