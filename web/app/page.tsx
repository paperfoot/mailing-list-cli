import Image from "next/image";
import { ContactForm } from "./components/ContactForm";

const proofPoints = [
  {
    value: "JSON",
    label: "every command emits structured output",
  },
  {
    value: "100",
    label: "recipient chunks with retry and resume",
  },
  {
    value: "HMAC",
    label: "signed unsubscribe links and one-click headers",
  },
];

const commandLines = [
  "$ mailing-list-cli broadcast send 42 --dry-run",
  "{ status: \"success\", projected_recipients: 18420 }",
  "$ mailing-list-cli template inspect launch",
  "{ verdict: \"email_ready\", lint_errors: 0 }",
  "$ mailing-list-cli unsubscribe sync --remote sharpclap",
  "{ synced: 31, suppressed: 31, cursor: 884 }",
];

const capabilities = [
  "Campaign drafts, approvals, previews, schedules, and resumable sends.",
  "Segments, tags, custom fields, suppression, erasure, and audit trails.",
  "Template linting, browser handoff inspection, and plain-text alternatives.",
  "Delivery, engagement, link, revenue, and complaint-rate reporting.",
];

export default function Home() {
  return (
    <main>
      <nav className="siteNav" aria-label="Primary navigation">
        <a className="navBrand" href="#top" aria-label="SharpClap home">
          <Image src="/sharpclap-mark.svg" alt="" width={38} height={38} priority />
          <div>
            <strong>SharpClap</strong>
            <span>agent-native mailing lists</span>
          </div>
        </a>
        <div className="navLinks">
          <a href="#engine">Engine</a>
          <a href="#inquiries">Inquiries</a>
        </div>
      </nav>

      <section id="top" className="hero">
        <div className="heroCopy">
          <p className="eyebrow">SharpClap for mailing-list-cli</p>
          <h1>The first high-performance, agent-native mailing list command center.</h1>
          <p className="heroText">
            SharpClap gives agents a fast, strict, JSON-speaking system for broadcasts, segments,
            templates, suppressions, unsubscribe handling, and reporting without forcing operators
            into a dashboard.
          </p>
          <div className="heroActions">
            <a className="primaryAction" href="#inquiries">
              Start the conversation
            </a>
            <a className="secondaryAction" href="#engine">
              See the engine
            </a>
          </div>
        </div>

        <aside className="terminalStage" aria-label="SharpClap command flow">
          <div className="signalRail" aria-hidden="true">
            <span />
            <span />
            <span />
          </div>
          <div className="terminalWindow">
            <div className="terminalTop">
              <span>sharpclap://ops</span>
              <span>live</span>
            </div>
            <pre>
              {commandLines.map((line) => (
                <code key={line}>{line}</code>
              ))}
            </pre>
          </div>
          <div className="miniLedger" aria-label="Operational guarantees">
            {proofPoints.map((point) => (
              <div key={point.value}>
                <strong>{point.value}</strong>
                <span>{point.label}</span>
              </div>
            ))}
          </div>
        </aside>
      </section>

      <section id="engine" className="engineSection">
        <div className="sectionHeading">
          <p className="eyebrow">Built for agents, supervised by operators</p>
          <h2>Everything important is explicit.</h2>
        </div>
        <div className="engineGrid">
          <article className="largePanel">
            <p className="panelKicker">No mystery state</p>
            <h3>Campaign work stays local, structured, and resumable.</h3>
            <p>
              Agents can create contacts, inspect templates, dry-run sends, sync unsubscribes,
              and read reports through one binary with semantic exit codes and predictable JSON.
            </p>
          </article>
          <div className="capabilityList">
            {capabilities.map((capability) => (
              <div className="capabilityRow" key={capability}>
                <span aria-hidden="true" />
                <p>{capability}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="unsubscribePreview" aria-label="Unsubscribe experience">
        <div>
          <p className="eyebrow">Compliance that still feels human</p>
          <h2>A clean opt-out page behind every campaign.</h2>
        </div>
        <div className="unsubscribeCard">
          <Image src="/sharpclap-mark.svg" alt="" width={44} height={44} />
          <h3>You are off the list.</h3>
          <p>
            SharpClap records the request immediately so future campaigns can skip that
            address before the next send.
          </p>
          <span>One-click compatible. Signed. Quiet.</span>
        </div>
      </section>

      <section id="inquiries" className="contactSection">
        <div className="contactCopy">
          <p className="eyebrow">Inquiries</p>
          <h2>Bring SharpClap into your agent workflow.</h2>
          <p>
            Tell us what you send, how your agents run, and where unsubscribe handling or
            reporting needs to plug in. The request goes into the private SharpClap intake queue.
          </p>
        </div>
        <ContactForm />
      </section>
    </main>
  );
}
