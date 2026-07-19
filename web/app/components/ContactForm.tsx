"use client";

import { FormEvent, useState } from "react";

type FormStatus = "idle" | "submitting" | "sent" | "error";

type FormState = {
  name: string;
  replyContact: string;
  organization: string;
  listSize: string;
  agentStack: string;
  message: string;
  website: string;
};

const initialState: FormState = {
  name: "",
  replyContact: "",
  organization: "",
  listSize: "",
  agentStack: "",
  message: "",
  website: "",
};

export function ContactForm() {
  const [form, setForm] = useState<FormState>(initialState);
  const [status, setStatus] = useState<FormStatus>("idle");
  const [error, setError] = useState("");

  function updateField(field: keyof FormState, value: string) {
    setForm((current) => ({ ...current, [field]: value }));
    if (status === "error") {
      setStatus("idle");
      setError("");
    }
  }

  async function submitInquiry(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setStatus("submitting");
    setError("");

    try {
      const response = await fetch("/api/inquiries", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(form),
      });
      const payload = (await response.json().catch(() => null)) as {
        status?: string;
        error?: { message?: string };
      } | null;

      if (!response.ok || payload?.status !== "success") {
        throw new Error(payload?.error?.message || "The inquiry could not be recorded.");
      }

      setForm(initialState);
      setStatus("sent");
    } catch (submitError) {
      setStatus("error");
      setError(submitError instanceof Error ? submitError.message : "The inquiry could not be recorded.");
    }
  }

  return (
    <form className="contactForm" onSubmit={submitInquiry}>
      <input
        aria-hidden="true"
        className="hpField"
        name="website"
        tabIndex={-1}
        autoComplete="off"
        value={form.website}
        onChange={(event) => updateField("website", event.target.value)}
      />

      <div className="fieldPair">
        <label>
          <span>Name</span>
          <input
            name="name"
            autoComplete="name"
            required
            maxLength={140}
            value={form.name}
            onChange={(event) => updateField("name", event.target.value)}
          />
        </label>
        <label>
          <span>Reply contact</span>
          <input
            name="replyContact"
            autoComplete="on"
            required
            maxLength={220}
            value={form.replyContact}
            onChange={(event) => updateField("replyContact", event.target.value)}
          />
        </label>
      </div>

      <div className="fieldPair">
        <label>
          <span>Organization</span>
          <input
            name="organization"
            maxLength={180}
            value={form.organization}
            onChange={(event) => updateField("organization", event.target.value)}
          />
        </label>
        <label>
          <span>List size</span>
          <select
            name="listSize"
            value={form.listSize}
            onChange={(event) => updateField("listSize", event.target.value)}
          >
            <option value="">Choose one</option>
            <option value="under_1000">Under 1,000</option>
            <option value="1000_10000">1,000 to 10,000</option>
            <option value="10000_100000">10,000 to 100,000</option>
            <option value="over_100000">Over 100,000</option>
          </select>
        </label>
      </div>

      <label>
        <span>Agent stack</span>
        <input
          name="agentStack"
          maxLength={220}
          placeholder="Codex, Claude, Gemini, custom runners"
          value={form.agentStack}
          onChange={(event) => updateField("agentStack", event.target.value)}
        />
      </label>

      <label>
        <span>What should SharpClap handle?</span>
        <textarea
          name="message"
          required
          maxLength={4000}
          rows={6}
          value={form.message}
          onChange={(event) => updateField("message", event.target.value)}
        />
      </label>

      <div className="formFooter">
        <button type="submit" disabled={status === "submitting"}>
          {status === "submitting" ? "Recording..." : "Send inquiry"}
        </button>
        <p aria-live="polite" className={status === "error" ? "formNote errorNote" : "formNote"}>
          {status === "sent"
            ? "Inquiry recorded. We will respond through the contact details you provided."
            : status === "error"
              ? error
              : "Use this for product, integration, or deployment inquiries."}
        </p>
      </div>
    </form>
  );
}
