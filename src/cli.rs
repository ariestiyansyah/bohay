//! CLI client (M4): `bohay pane …` / `bohay ping` / `bohay events` connect to
//! the session socket, send one JSON request, and print the reply. See docs/08.

use std::io::{BufRead, BufReader, Write};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Returns true if `argv[1]` is a CLI noun we handle (so `main` should not
/// launch the TUI).
pub fn is_cli(args: &[String]) -> bool {
    matches!(
        args.get(1).map(String::as_str),
        Some("ping" | "pane" | "node" | "tab" | "agent" | "ui" | "events" | "module" | "help")
    )
}

const USAGE: &str = "\
bohay — terminal workspace manager for AI coding agents

usage: bohay <command> [args]

  (no args)            launch / attach the TUI
  help                 show this help
  ping                 check the server

nodes (spaces):
  node list                  list nodes
  node new                   create a node in the current directory
  node focus <i>             focus node i (0-based)
  node close [<i>]           close a node (default: active)

tabs:
  tab list                   list tabs in the current node
  tab new                    new tab
  tab focus <n>              focus tab n (1-based)
  tab close [<n>]            close a tab (default: active)

panes / agents:
  pane list                  list panes in the current tab
  pane split [<id>] [--down] split a pane (default: side by side)
  pane focus <id>            focus a pane (jumps to its node/tab)
  pane run [<id>] <cmd...>   run a command in a pane
  pane send [<id>] <text>    send raw text to a pane
  pane read [<id>]           print a pane's recent output
  pane close [<id>]          close a pane
  agent list                 list every agent across all nodes/tabs
  agent sessions             list resumable sessions found on disk
  agent resume <id>          reopen a resumable session into a pane

appearance:
  ui sidebar --width <n>     set the sidebar width (columns)
  ui sidebar --hide|--show   toggle the sidebar

modules (extensions — docs/13):
  module search [<query>]    find modules published to the `bohay-module` GitHub topic
  module list                list installed modules
  module info <id>           show a module's actions / panes / events / source
  module link <path>         register a local module dir (--disabled to skip enabling)
  module install <owner>/<repo>[/sub] [--ref REF] [--yes]   install from GitHub
  module unlink <id>         remove a module from the registry
  module uninstall <id>      unlink + delete a git-installed module's checkout
  module enable <id> | disable <id>
  module actions             list every action across modules
  module run <id> <action>   invoke a module action (captures + logs output)
  module pane open <id> <entrypoint> [--placement split|overlay|tab]
  module pane focus <pane> | close <pane>
  module log [<id>]          tail module command logs (--limit N)
  module config-dir <id>     print/create a module's config dir

events:
  events                     stream live status changes

server:
  server stop                stop the server (and all panes)
  integration install claude install the claude resume hook
";

pub fn run(args: &[String]) -> Result<i32> {
    if args.get(1).map(String::as_str) == Some("help") {
        print!("{USAGE}");
        return Ok(0);
    }
    // `module install` clones + builds locally (with a confirm prompt), then
    // registers over the socket — it isn't a plain request/response.
    if args.get(1).map(String::as_str) == Some("module")
        && args.get(2).map(String::as_str) == Some("install")
    {
        return module_install(args);
    }
    // `module search` is a read-only GitHub lookup — no server involved.
    if args.get(1).map(String::as_str) == Some("module")
        && args.get(2).map(String::as_str) == Some("search")
    {
        return module_search(args);
    }
    let (method, params) = parse(args)?;
    let path = crate::persist::socket_path();
    let mut stream = crate::ipc::transport::connect(&path)
        .map_err(|_| anyhow!("no bohay server running (socket: {})", path.display()))?;

    let req = json!({ "id": "1", "method": method, "params": params });
    writeln!(stream, "{req}")?;

    let mut reader = BufReader::new(stream);
    if method == "events.subscribe" {
        // Stream events until the connection closes.
        for line in reader.lines() {
            match line {
                Ok(l) => println!("{l}"),
                Err(_) => break,
            }
        }
        return Ok(0);
    }

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let line = line.trim();
    // Pretty-print and set exit code on error.
    match serde_json::from_str::<Value>(line) {
        Ok(v) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| line.to_string())
            );
            if v.get("error").is_some() {
                return Ok(1);
            }
        }
        Err(_) => println!("{line}"),
    }
    Ok(0)
}

/// `bohay module install owner/repo[/sub] [--ref REF] [--yes]` — clone + build
/// locally, then register over the socket (or directly if the server is down).
fn module_install(args: &[String]) -> Result<i32> {
    let spec = args
        .get(3)
        .filter(|s| !s.starts_with("--"))
        .ok_or_else(|| {
            anyhow!("usage: bohay module install owner/repo[/sub] [--ref REF] [--yes]")
        })?;
    let git_ref = flag(args, "--ref");
    let yes = args.iter().any(|a| a == "--yes" || a == "-y");

    let installed = crate::module::install::install(spec, git_ref.as_deref(), yes)?;
    let params = json!({
        "path": installed.root.display().to_string(),
        "source": installed.source,
    });
    match send_request("module.link", params) {
        Ok(v) if v.get("error").is_some() => {
            // e.g. already registered — leave the checkout but report it.
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            Ok(1)
        }
        Ok(_) => {
            println!("installed {} ({})", installed.id, installed.source);
            Ok(0)
        }
        Err(_) => {
            // Server down: write the registry directly; it loads on next start.
            register_directly(&installed)?;
            println!(
                "installed {} ({}) — start bohay to use it",
                installed.id, installed.source
            );
            Ok(0)
        }
    }
}

/// `bohay module search [<query>]` — list modules published to the
/// `bohay-module` GitHub topic. Read-only; doesn't need a running server.
fn module_search(args: &[String]) -> Result<i32> {
    let terms: Vec<&str> = args
        .get(3..)
        .unwrap_or(&[])
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect();
    let query = (!terms.is_empty()).then(|| terms.join(" "));

    let hits = crate::module::discovery::search(query.as_deref())?;
    if hits.is_empty() {
        println!("No modules found in the `bohay-module` topic yet.");
        println!("Publish one by tagging a public repo with the `bohay-module` topic.");
        return Ok(0);
    }
    for h in &hits {
        println!("  ★ {:<5} {}", h.stars, h.full_name);
        if !h.description.is_empty() {
            println!("          {}", h.description);
        }
        if !h.url.is_empty() {
            println!("          {}", h.url);
        }
    }
    println!(
        "\n{} result(s). Install with:  bohay module install <owner>/<repo>",
        hits.len()
    );
    Ok(0)
}

/// One request/response over the control socket.
fn send_request(method: &str, params: Value) -> Result<Value> {
    let path = crate::persist::socket_path();
    let mut stream =
        crate::ipc::transport::connect(&path).map_err(|_| anyhow!("no bohay server running"))?;
    let req = json!({ "id": "1", "method": method, "params": params });
    writeln!(stream, "{req}")?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    serde_json::from_str(line.trim()).map_err(|e| anyhow!("bad reply: {e}"))
}

/// Register an installed module by writing the registry file directly (used when
/// no server is running).
fn register_directly(installed: &crate::module::install::Installed) -> Result<()> {
    use crate::module::{manifest::ModuleManifest, registry, InstalledModule};
    let mut reg = registry::load();
    reg.modules.retain(|m| m.id != installed.id);
    let manifest = ModuleManifest::load(&installed.root).map_err(|e| anyhow!(e))?;
    reg.modules.push(InstalledModule {
        id: installed.id.clone(),
        root: installed.root.clone(),
        enabled: true,
        source: Some(installed.source.clone()),
        manifest,
        warning: None,
    });
    registry::save(&reg);
    Ok(())
}

/// Map argv to a `(method, params)` pair.
fn parse(args: &[String]) -> Result<(String, Value)> {
    let noun = args.get(1).map(String::as_str).unwrap_or("");
    let verb = args.get(2).map(String::as_str).unwrap_or("");
    let rest = &args[3.min(args.len())..];

    // The pane id is the first numeric positional, else $BOHAY_PANE_ID.
    let pane = || -> Value {
        if let Some(first) = rest.first() {
            if first.parse::<u32>().is_ok() {
                return json!(first);
            }
        }
        match std::env::var("BOHAY_PANE_ID") {
            Ok(v) => json!(v),
            Err(_) => Value::Null,
        }
    };
    // Args after an optional leading numeric pane id.
    let tail = || -> Vec<String> {
        let skip = rest
            .first()
            .map(|s| s.parse::<u32>().is_ok())
            .unwrap_or(false);
        rest[if skip { 1 } else { 0 }..].to_vec()
    };

    let with_pane = |mut obj: serde_json::Map<String, Value>| {
        let p = pane();
        if !p.is_null() {
            obj.insert("pane".to_string(), p);
        }
        Value::Object(obj)
    };

    // First positional arg after the verb (for node/tab indices).
    let arg0 = || rest.first().cloned();
    let one = |key: &str, val: Option<String>| {
        let mut obj = serde_json::Map::new();
        if let Some(v) = val {
            obj.insert(key.to_string(), json!(v));
        }
        Value::Object(obj)
    };

    Ok(match (noun, verb) {
        ("ping", _) => ("ping".into(), json!({})),
        ("events", _) => ("events.subscribe".into(), json!({})),
        ("agent", "sessions") => ("agent.sessions".into(), json!({})),
        ("agent", "resume") => ("agent.resume".into(), one("session_id", arg0())),
        ("agent", _) => ("agent.list".into(), json!({})),

        ("ui", "sidebar") => {
            let mut obj = serde_json::Map::new();
            if let Some(w) = flag(args, "--width") {
                obj.insert("width".to_string(), json!(w));
            }
            if args.iter().any(|a| a == "--hide") {
                obj.insert("visible".to_string(), json!(false));
            } else if args.iter().any(|a| a == "--show") {
                obj.insert("visible".to_string(), json!(true));
            }
            ("ui.sidebar".into(), Value::Object(obj))
        }

        ("node", "new") => ("node.new".into(), json!({})),
        ("node", "focus") => ("node.focus".into(), one("node", arg0())),
        ("node", "close") => ("node.close".into(), one("node", arg0())),
        ("node", _) => ("node.list".into(), json!({})),

        ("tab", "new") => ("tab.new".into(), json!({})),
        ("tab", "focus") => ("tab.focus".into(), one("tab", arg0())),
        ("tab", "close") => ("tab.close".into(), one("tab", arg0())),
        ("tab", _) => ("tab.list".into(), json!({})),

        ("pane", "split") => {
            let mut obj = serde_json::Map::new();
            if args.iter().any(|a| a == "--down" || a == "--stack") {
                obj.insert("direction".to_string(), json!("down"));
            }
            ("pane.split".into(), with_pane(obj))
        }
        ("pane", "focus") => ("pane.focus".into(), with_pane(serde_json::Map::new())),
        ("pane", "run") => {
            let command = tail().join(" ");
            let mut obj = serde_json::Map::new();
            obj.insert("command".to_string(), json!(command));
            ("pane.run".into(), with_pane(obj))
        }
        ("pane", "send") => {
            let text = tail().join(" ");
            let mut obj = serde_json::Map::new();
            obj.insert("text".to_string(), json!(text));
            ("pane.send_input".into(), with_pane(obj))
        }
        ("pane", "read") => ("pane.read".into(), with_pane(serde_json::Map::new())),
        ("pane", "close") => ("pane.close".into(), with_pane(serde_json::Map::new())),
        ("pane", "report") => {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "agent".to_string(),
                json!(flag(args, "--agent").unwrap_or_default()),
            );
            obj.insert(
                "session_id".to_string(),
                json!(flag(args, "--session").unwrap_or_default()),
            );
            ("pane.report_session".into(), with_pane(obj))
        }
        ("pane", _) => ("pane.list".into(), json!({})),

        ("module", "link") => {
            let mut obj = serde_json::Map::new();
            if let Some(path) = rest.first() {
                obj.insert("path".to_string(), json!(path));
            }
            if args.iter().any(|a| a == "--disabled") {
                obj.insert("disabled".to_string(), json!(true));
            }
            ("module.link".into(), Value::Object(obj))
        }
        ("module", "unlink") => ("module.unlink".into(), one("id", arg0())),
        ("module", "uninstall") => ("module.uninstall".into(), one("id", arg0())),
        ("module", "enable") => ("module.enable".into(), one("id", arg0())),
        ("module", "disable") => ("module.disable".into(), one("id", arg0())),
        ("module", "run") => {
            let mut obj = serde_json::Map::new();
            match (rest.first(), rest.get(1)) {
                (Some(m), Some(a)) => {
                    obj.insert("module".to_string(), json!(m));
                    obj.insert("id".to_string(), json!(a));
                }
                (Some(a), None) => {
                    obj.insert("id".to_string(), json!(a));
                }
                _ => return Err(anyhow!("usage: bohay module run <module> <action>")),
            }
            ("module.action.invoke".into(), Value::Object(obj))
        }
        ("module", "actions") => ("module.action.list".into(), json!({})),
        ("module", "log") => {
            let mut obj = serde_json::Map::new();
            if let Some(id) = rest.first().filter(|s| !s.starts_with("--")) {
                obj.insert("id".to_string(), json!(id));
            }
            if let Some(n) = flag(args, "--limit").and_then(|s| s.parse::<u64>().ok()) {
                obj.insert("limit".to_string(), json!(n));
            }
            ("module.log.list".into(), Value::Object(obj))
        }
        ("module", "info") => ("module.info".into(), one("id", arg0())),
        ("module", "config-dir") => ("module.config_dir".into(), one("id", arg0())),
        ("module", "pane") => {
            let sub = rest.first().map(String::as_str).unwrap_or("");
            match sub {
                "open" => {
                    let mut obj = serde_json::Map::new();
                    if let Some(m) = rest.get(1) {
                        obj.insert("module".to_string(), json!(m));
                    }
                    if let Some(e) = rest.get(2) {
                        obj.insert("entrypoint".to_string(), json!(e));
                    }
                    if let Some(pl) = flag(args, "--placement") {
                        obj.insert("placement".to_string(), json!(pl));
                    }
                    ("module.pane.open".into(), Value::Object(obj))
                }
                "focus" => (
                    "module.pane.focus".into(),
                    one("pane", rest.get(1).cloned()),
                ),
                "close" => (
                    "module.pane.close".into(),
                    one("pane", rest.get(1).cloned()),
                ),
                _ => return Err(anyhow!("usage: bohay module pane open|focus|close …")),
            }
        }
        ("module", _) => ("module.list".into(), json!({})),

        _ => return Err(anyhow!("unknown command. Try `bohay help`.")),
    })
}

/// Value following `--name` in argv, if present.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn maps_commands() {
        std::env::remove_var("BOHAY_PANE_ID");
        let (m, _) = parse(&argv("bohay ping")).unwrap();
        assert_eq!(m, "ping");

        let (m, _) = parse(&argv("bohay pane list")).unwrap();
        assert_eq!(m, "pane.list");

        let (m, p) = parse(&argv("bohay pane split --down")).unwrap();
        assert_eq!(m, "pane.split");
        assert_eq!(p.get("direction").and_then(|v| v.as_str()), Some("down"));

        let (m, p) = parse(&argv("bohay pane run 3 echo hi")).unwrap();
        assert_eq!(m, "pane.run");
        assert_eq!(p.get("pane").and_then(|v| v.as_str()), Some("3"));
        assert_eq!(p.get("command").and_then(|v| v.as_str()), Some("echo hi"));

        let (m, _) = parse(&argv("bohay node list")).unwrap();
        assert_eq!(m, "node.list");
        let (m, p) = parse(&argv("bohay node focus 2")).unwrap();
        assert_eq!(m, "node.focus");
        assert_eq!(p.get("node").and_then(|v| v.as_str()), Some("2"));
        let (m, _) = parse(&argv("bohay tab new")).unwrap();
        assert_eq!(m, "tab.new");
        let (m, _) = parse(&argv("bohay agent list")).unwrap();
        assert_eq!(m, "agent.list");
    }

    #[test]
    fn maps_module_commands() {
        let (m, _) = parse(&argv("bohay module list")).unwrap();
        assert_eq!(m, "module.list");

        let (m, p) = parse(&argv("bohay module link ./mod --disabled")).unwrap();
        assert_eq!(m, "module.link");
        assert_eq!(p.get("path").and_then(|v| v.as_str()), Some("./mod"));
        assert_eq!(p.get("disabled").and_then(|v| v.as_bool()), Some(true));

        let (m, p) = parse(&argv("bohay module run my-mod refresh")).unwrap();
        assert_eq!(m, "module.action.invoke");
        assert_eq!(p.get("module").and_then(|v| v.as_str()), Some("my-mod"));
        assert_eq!(p.get("id").and_then(|v| v.as_str()), Some("refresh"));

        let (m, p) = parse(&argv("bohay module run refresh")).unwrap();
        assert_eq!(m, "module.action.invoke");
        assert_eq!(p.get("id").and_then(|v| v.as_str()), Some("refresh"));
        assert!(p.get("module").is_none());

        let (m, p) = parse(&argv("bohay module enable my-mod")).unwrap();
        assert_eq!(m, "module.enable");
        assert_eq!(p.get("id").and_then(|v| v.as_str()), Some("my-mod"));
    }
}
