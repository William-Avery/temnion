// SPDX-License-Identifier: AGPL-3.0-only

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { type AppendRequest, appendEvent } from "../../api";
import { errorText } from "../../utils/formatters";
import { Notice, PanelHeader } from "../common";

export function IngestPanel({ connected }: { connected: boolean }) {
  const queryClient = useQueryClient();
  const [form, setForm] = useState<AppendRequest>({
    entity: "0:1:0",
    schema: 1,
    validTime: "1:0",
    knownTime: "1:0",
    payloadHex: "00",
    causes: [],
  });
  const [causes, setCauses] = useState("");
  const append = useMutation({
    mutationFn: () => appendEvent({ ...form, causes: causes.split(",").map((cause) => cause.trim()).filter(Boolean) }),
    onSuccess: (receipt) => {
      queryClient.setQueryData(["engine-status"], receipt.status);
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      void queryClient.invalidateQueries({ queryKey: ["branches"] });
    },
  });
  const update = <K extends keyof AppendRequest>(key: K, value: AppendRequest[K]) => setForm((current) => ({ ...current, [key]: value }));
  return (
    <section className="view-panel active">
      <PanelHeader title="Deterministic ingestion" subtitle="Validate and append one bounded event; success is reported only after the durable store acknowledges OS synchronization." />
      {!connected && <Notice>Connect a database before appending an event.</Notice>}
      {append.error && <Notice kind="error">{errorText(append.error)}</Notice>}
      {append.data && <Notice kind="success">Committed {append.data.count} event as {append.data.firstEvent}.</Notice>}
      <div className="glass-card ingest-form-card">
        <div className="form-grid">
          <label className="form-group">Entity (shard:slot:generation)<input className="text-input" value={form.entity} onChange={(event) => update("entity", event.target.value)} /></label>
          <label className="form-group">Schema ID<input className="num-input" min={0} type="number" value={form.schema} onChange={(event) => update("schema", Number(event.target.value))} /></label>
          <label className="form-group">Valid time (clock:tick)<input className="text-input" value={form.validTime} onChange={(event) => update("validTime", event.target.value)} /></label>
          <label className="form-group">Known time (clock:tick)<input className="text-input" value={form.knownTime} onChange={(event) => update("knownTime", event.target.value)} /></label>
        </div>
        <label className="form-group">Cause IDs (source:epoch:sequence, comma-separated)<input className="text-input" value={causes} onChange={(event) => setCauses(event.target.value)} /></label>
        <label className="form-group">Hex payload<textarea className="code-editor compact-editor" spellCheck={false} value={form.payloadHex} onChange={(event) => update("payloadHex", event.target.value)} /></label>
        <div className="form-actions">
          <button className="btn btn-primary" disabled={!connected || append.isPending} onClick={() => append.mutate()} type="button">
            {append.isPending ? "Committing…" : "Admit & commit event"}
          </button>
        </div>
      </div>
    </section>
  );
}
