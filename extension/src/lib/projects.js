// A project list is what the server returns plus the Inbox fallback. Keeping the choice list in
// one place means the popup, the options page and the tests cannot disagree about the first row
// or about a project the server no longer lists.
export function projectChoices(projects, current = "") {
  const choices = [{ value: "", label: "Inbox" }];
  const seen = new Set([""]);
  for (const project of projects || []) {
    const value = String(project?.slug || "").trim();
    // `/api/projects` includes the virtual Inbox (`inbox`); the empty value already means it.
    if (!value || value === "inbox" || seen.has(value)) continue;
    seen.add(value);
    choices.push({ value, label: String(project.name || value) });
  }
  // A saved project may have disappeared from the server (or the list may have failed to load);
  // keep it selectable instead of silently switching the human back to Inbox.
  const selected = current === "inbox" ? "" : String(current || "");
  if (selected && !seen.has(selected)) choices.push({ value: selected, label: selected });
  return choices;
}

export function fillProjectSelect(select, projects, current = "") {
  const selected = current === "inbox" ? "" : String(current || "");
  select.innerHTML = "";
  for (const choice of projectChoices(projects, selected)) {
    const option = document.createElement("option");
    option.value = choice.value;
    option.textContent = choice.label;
    select.append(option);
  }
  select.value = selected;
  return select.value;
}
