// SPDX-License-Identifier: AGPL-3.0-only

import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";

import { type BranchInfo, listBranches, traceCausality } from "../../api";
import { errorText } from "../../utils/formatters";
import { EmptyState, Notice, PanelHeader } from "../common";

export function BranchCard({ branch }: { branch: BranchInfo }) {
  return (
    <div className="branch-item">
      <div className="b-name">{branch.name} <span className="badge badge-neutral">{branch.lifecycle}</span></div>
      <div className="b-meta">
        ID {branch.id} · {branch.parentId === undefined ? "root" : `parent ${branch.parentId} at sequence ${branch.forkSequence}`}
      </div>
    </div>
  );
}

export function CausalityPanel({ connected, eventCount }: { connected: boolean; eventCount: number }) {
  const [sequence, setSequence] = useState(Math.max(0, eventCount - 1));
  const [depth, setDepth] = useState(8);
  const branches = useQuery({ queryKey: ["branches", eventCount], queryFn: listBranches, enabled: connected });
  const trace = useMutation({ mutationFn: () => traceCausality(sequence, depth) });
  return (
    <section className="view-panel active">
      <PanelHeader title="Branches & causal trace" subtitle="Read persistent branch metadata and trace declared causal edges from bounded durable history." />
      {!connected && <Notice>Connect a database to inspect branch or causal metadata.</Notice>}
      {(branches.error || trace.error) && <Notice kind="error">{errorText(branches.error ?? trace.error)}</Notice>}
      <div className="causal-layout">
        <div className="glass-card branch-list-card">
          <div className="card-title">Timeline branches</div>
          {(branches.data ?? []).map((branch) => <BranchCard branch={branch} key={branch.id} />)}
          {!branches.data?.length && <EmptyState>No branch metadata loaded.</EmptyState>}
        </div>
        <div className="glass-card causal-graph-card">
          <div className="card-title">Trace an event</div>
          <div className="inline-form">
            <label>Sequence<input className="num-input" min={0} type="number" value={sequence} onChange={(event) => setSequence(Number(event.target.value))} /></label>
            <label>Depth<input className="num-input" min={1} max={32} type="number" value={depth} onChange={(event) => setDepth(Number(event.target.value))} /></label>
            <button className="btn btn-primary" disabled={!connected || trace.isPending} onClick={() => trace.mutate()} type="button">Trace</button>
          </div>
          {trace.data ? (
            <div className="trace-grid">
              <div><span className="eyebrow">Root</span><code>{trace.data.root}</code></div>
              <div><span className="eyebrow">Upstream causes</span>{trace.data.causes.map((node) => <code key={node.eventId}>{node.eventId} · depth {node.depth}</code>)}</div>
              <div><span className="eyebrow">Downstream effects</span>{trace.data.effects.map((node) => <code key={node.eventId}>{node.eventId} · depth {node.depth}</code>)}</div>
              <div><span className="eyebrow">Edges</span>{trace.data.edges.map((edge) => <code key={edge}>{edge}</code>)}</div>
            </div>
          ) : <EmptyState>Enter an existing source-local sequence to inspect its causal cone.</EmptyState>}
        </div>
      </div>
    </section>
  );
}
