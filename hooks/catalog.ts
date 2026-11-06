// Generated from `sn introspect` by tests/plugin_catalog.rs. Do not edit:
// regenerate with `UPDATE_PLUGIN_CATALOG=1 cargo test --test plugin_catalog`.

export type CatalogNode = {
  // The positional arguments' names, in order.
  operands: readonly string[]
  // Long options (aliases included) that consume the next word.
  values: readonly string[]
  // Short options that consume the next word, one letter each.
  shorts: string
  sub: Readonly<Record<string, CatalogNode>>
}

export const GLOBAL_VALUES: readonly string[] = ["ca-cert","output","profile","proxy","proxy-ca-cert","timeout"]
export const GLOBAL_SHORTS = "p"

export const CATALOG: CatalogNode = {
  operands: [],
  values: [],
  shorts: "",
  sub: {
    "init": { operands: [], values: ["api-key","auth","client-id","client-secret","grant","instance","jwt-alg","jwt-key-file","jwt-kid","jwt-subject","password","profile","redirect-uri","token","token-command","username"], shorts: "", sub: {} },
    "profile": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "add": { operands: ["name"], values: ["api-key","auth","client-id","client-secret","grant","instance","jwt-alg","jwt-key-file","jwt-kid","jwt-subject","password","redirect-uri","token","token-command","username"], shorts: "", sub: {} },
        "list": { operands: [], values: [], shorts: "", sub: {} },
        "show": { operands: ["name"], values: [], shorts: "", sub: {} },
        "remove": { operands: ["name"], values: [], shorts: "", sub: {} },
        "use": { operands: ["name"], values: [], shorts: "", sub: {} },
        "login": { operands: [], values: [], shorts: "", sub: {} },
        "logout": { operands: [], values: [], shorts: "", sub: {} },
        "status": { operands: [], values: [], shorts: "", sub: {} },
        "refresh": { operands: [], values: [], shorts: "", sub: {} },
      },
    },
    "table": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: ["table"], values: ["display-value","fields","limit","max-records","offset","page-size","paginate","query","query-category","resume-from","setLimit","setlimit","sysparm-display-value","sysparm-fields","sysparm-limit","sysparm-offset","sysparm-query","sysparm-query-category","sysparm-view","view"], shorts: "fq", sub: {} },
        "get": { operands: ["table","sys_id"], values: ["display-value","fields","sysparm-display-value","sysparm-fields","sysparm-view","view"], shorts: "f", sub: {} },
        "create": { operands: ["table"], values: ["data","display-value","field","fields","sysparm-display-value","sysparm-fields","sysparm-view","view"], shorts: "DFf", sub: {} },
        "update": { operands: ["table","sys_id"], values: ["data","display-value","field","fields","sysparm-display-value","sysparm-fields","sysparm-view","view"], shorts: "DFf", sub: {} },
        "delete": { operands: ["table","sys_id"], values: [], shorts: "", sub: {} },
      },
    },
    "get": {
      operands: ["reference"],
      values: ["display-value","fields","sysparm-display-value","sysparm-fields","sysparm-view","view"],
      shorts: "f",
      sub: {
        "scope": { operands: [], values: [], shorts: "", sub: {} },
        "updateset": { operands: ["set"], values: ["limit"], shorts: "l", sub: {} },
        "property": { operands: ["name"], values: ["scope"], shorts: "", sub: {} },
        "script": { operands: ["name"], values: ["scope","type"], shorts: "t", sub: {} },
        "choices": { operands: ["column","field"], values: [], shorts: "", sub: {} },
        "plugin": { operands: ["ids"], values: [], shorts: "", sub: {} },
        "me": { operands: [], values: [], shorts: "", sub: {} },
        "instance": { operands: [], values: [], shorts: "", sub: {} },
        "mine": { operands: [], values: ["limit","table"], shorts: "l", sub: {} },
        "last": { operands: [], values: ["fields","table"], shorts: "f", sub: {} },
        "logs": { operands: [], values: ["grep","level","limit","since","source"], shorts: "gl", sub: {} },
      },
    },
    "set": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "scope": { operands: ["target"], values: [], shorts: "", sub: {} },
        "updateset": { operands: ["target"], values: [], shorts: "", sub: {} },
      },
    },
    "journal": { operands: ["table","sys_id"], values: ["limit","source"], shorts: "", sub: {} },
    "variables": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "get": { operands: ["table","sys_id"], values: [], shorts: "", sub: {} },
        "set": { operands: ["table","sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
      },
    },
    "watch": { operands: ["table"], values: ["duration","idle-timeout","max-events","on-change","operation","query","session-rotate","sysparm-query","watch-fields"], shorts: "q", sub: {} },
    "schema": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "tables": { operands: [], values: ["filter"], shorts: "", sub: {} },
        "columns": { operands: ["table"], values: ["filter","type"], shorts: "", sub: {} },
        "choices": { operands: ["table","field"], values: [], shorts: "", sub: {} },
      },
    },
    "cache": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "refresh": { operands: [], values: [], shorts: "", sub: {} },
        "status": { operands: [], values: [], shorts: "", sub: {} },
        "tables": { operands: ["prefix"], values: [], shorts: "", sub: {} },
        "columns": { operands: ["table"], values: [], shorts: "", sub: {} },
      },
    },
    "api": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: [], values: ["namespace"], shorts: "n", sub: {} },
        "search": { operands: ["term"], values: ["method","namespace"], shorts: "mn", sub: {} },
        "spec": { operands: ["name"], values: ["format","namespace","version"], shorts: "n", sub: {} },
      },
    },
    "codesearch": { operands: ["term"], values: ["limit","scope","table"], shorts: "", sub: {} },
    "introspect": { operands: [], values: [], shorts: "", sub: {} },
    "cicd": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "app": {
          operands: [],
          values: [],
          shorts: "",
          sub: {
            "install": { operands: [], values: ["base-app-version","scope","sys-id","version","wait-timeout"], shorts: "", sub: {} },
            "publish": { operands: [], values: ["dev-notes","scope","sys-id","version","wait-timeout"], shorts: "", sub: {} },
            "rollback": { operands: [], values: ["scope","sys-id","version","wait-timeout"], shorts: "", sub: {} },
          },
        },
        "updateset": {
          operands: [],
          values: [],
          shorts: "",
          sub: {
            "create": { operands: [], values: ["description","name","scope","sys-id","update-set-name"], shorts: "", sub: {} },
            "retrieve": { operands: [], values: ["update-set-id","update-source-id","update-source-instance-id","wait-timeout"], shorts: "", sub: {} },
            "preview": { operands: ["remote_update_set_id"], values: ["wait-timeout"], shorts: "", sub: {} },
            "commit": { operands: ["remote_update_set_id"], values: ["wait-timeout"], shorts: "", sub: {} },
            "commit-multiple": { operands: [], values: ["ids","wait-timeout"], shorts: "", sub: {} },
            "back-out": { operands: [], values: ["update-set-id","wait-timeout"], shorts: "", sub: {} },
          },
        },
        "atf": {
          operands: [],
          values: [],
          shorts: "",
          sub: {
            "run": { operands: [], values: ["browser-name","browser-version","os-name","os-version","suite-id","suite-name","wait-timeout"], shorts: "", sub: {} },
            "results": { operands: ["result_id"], values: [], shorts: "", sub: {} },
          },
        },
        "progress": { operands: ["progress_id"], values: [], shorts: "", sub: {} },
      },
    },
    "context": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "scope": { operands: ["target"], values: [], shorts: "", sub: {} },
        "updateset": { operands: ["target"], values: [], shorts: "", sub: {} },
      },
    },
    "flow": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: [], values: ["fields","limit","offset","query","scope","setLimit","setlimit","status","sysparm-fields","sysparm-limit","sysparm-offset","sysparm-query","type"], shorts: "fq", sub: {} },
        "get": { operands: ["flow"], values: [], shorts: "", sub: {} },
        "versions": { operands: ["flow"], values: [], shorts: "", sub: {} },
        "runs": { operands: ["flow"], values: ["limit","record","setLimit","setlimit","since","state","sysparm-limit"], shorts: "", sub: {} },
        "debug": { operands: ["context"], values: ["log-lines"], shorts: "", sub: {} },
        "steps": { operands: ["context"], values: [], shorts: "", sub: {} },
        "logs": { operands: ["context"], values: ["level","limit","setLimit","setlimit","sysparm-limit"], shorts: "", sub: {} },
        "why-not": { operands: ["flow"], values: ["record"], shorts: "", sub: {} },
        "tail": { operands: ["flow"], values: ["duration","idle-timeout","max-events"], shorts: "", sub: {} },
      },
    },
    "aggregate": { operands: ["table"], values: ["avg-fields","display-value","group-by","having","max-fields","min-fields","order-by","query","query-category","sum-fields","sysparm-avg-fields","sysparm-display-value","sysparm-group-by","sysparm-having","sysparm-max-fields","sysparm-min-fields","sysparm-order-by","sysparm-query","sysparm-query-category","sysparm-sum-fields"], shorts: "q", sub: {} },
    "scores": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: [], values: ["breakdown","breakdown-relation","contains","display","display-value","elements-filter","from","limit","page","per-page","setLimit","sort-by","sort-dir","step","sysparm-breakdown","sysparm-breakdown-relation","sysparm-contains","sysparm-display","sysparm-display-value","sysparm-elements-filter","sysparm-from","sysparm-limit","sysparm-page","sysparm-per-page","sysparm-sortby","sysparm-sortdir","sysparm-step","sysparm-tags","sysparm-to","sysparm-uuid","tags","to","uuid"], shorts: "", sub: {} },
        "favorite": { operands: ["uuid"], values: [], shorts: "", sub: {} },
        "unfavorite": { operands: ["uuid"], values: [], shorts: "", sub: {} },
      },
    },
    "change": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: [], values: ["display-value","fields","limit","offset","query","setLimit","setlimit","sysparm-display-value","sysparm-fields","sysparm-limit","sysparm-offset","sysparm-query","sysparm-view","type","view"], shorts: "fq", sub: {} },
        "get": { operands: ["sys_id"], values: ["display-value","fields","sysparm-display-value","sysparm-fields","sysparm-view","type","view"], shorts: "f", sub: {} },
        "create": { operands: [], values: ["data","display-value","field","fields","sysparm-display-value","sysparm-fields","template","type"], shorts: "DFf", sub: {} },
        "update": { operands: ["sys_id"], values: ["data","display-value","field","fields","sysparm-display-value","sysparm-fields","type"], shorts: "DFf", sub: {} },
        "delete": { operands: ["sys_id"], values: ["type"], shorts: "", sub: {} },
        "nextstates": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "approvals": { operands: ["sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
        "risk": { operands: ["sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
        "schedule": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "task": {
          operands: [],
          values: [],
          shorts: "",
          sub: {
            "list": { operands: ["change_sys_id"], values: ["fields","limit","setLimit","setlimit","sysparm-fields","sysparm-limit"], shorts: "f", sub: {} },
            "get": { operands: ["change_sys_id","task_sys_id"], values: [], shorts: "", sub: {} },
            "create": { operands: ["change_sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
            "update": { operands: ["change_sys_id","task_sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
            "delete": { operands: ["change_sys_id","task_sys_id"], values: [], shorts: "", sub: {} },
          },
        },
        "ci": {
          operands: [],
          values: [],
          shorts: "",
          sub: {
            "list": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
            "add": { operands: ["change_sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
          },
        },
        "conflict": {
          operands: [],
          values: [],
          shorts: "",
          sub: {
            "get": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
            "add": { operands: ["sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
            "remove": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
          },
        },
        "models": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "templates": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
      },
    },
    "attachment": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: [], values: ["limit","offset","query","setLimit","setlimit","sysparm-limit","sysparm-offset","sysparm-query"], shorts: "q", sub: {} },
        "get": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "upload": { operands: [], values: ["content-type","encryption-context","file","file-name","record","table"], shorts: "", sub: {} },
        "download": { operands: ["sys_id"], values: ["out"], shorts: "o", sub: {} },
        "delete": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
      },
    },
    "cmdb": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: ["class"], values: ["limit","offset","query","setLimit","setlimit","sysparm-limit","sysparm-offset","sysparm-query"], shorts: "q", sub: {} },
        "get": { operands: ["class","sys_id"], values: [], shorts: "", sub: {} },
        "create": { operands: ["class"], values: ["data","field","source"], shorts: "DF", sub: {} },
        "update": { operands: ["class","sys_id"], values: ["data","field","source"], shorts: "DF", sub: {} },
        "meta": { operands: ["class"], values: [], shorts: "", sub: {} },
        "relation": {
          operands: [],
          values: [],
          shorts: "",
          sub: {
            "add": { operands: ["class","sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
            "delete": { operands: ["class","sys_id","rel_sys_id"], values: [], shorts: "", sub: {} },
          },
        },
      },
    },
    "import": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "create": { operands: ["staging_table"], values: ["data","field"], shorts: "DF", sub: {} },
        "bulk": { operands: ["staging_table"], values: ["data"], shorts: "D", sub: {} },
        "get": { operands: ["staging_table","sys_id"], values: [], shorts: "", sub: {} },
      },
    },
    "xml": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "export": { operands: ["table","query"], values: ["limit"], shorts: "l", sub: {} },
        "import": { operands: ["file"], values: [], shorts: "", sub: {} },
      },
    },
    "catalog": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: [], values: ["sysparm-text","text"], shorts: "", sub: {} },
        "get": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "categories": { operands: ["catalog_sys_id"], values: ["limit","offset","setLimit","setlimit","sysparm-limit","sysparm-offset"], shorts: "", sub: {} },
        "category": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "items": { operands: [], values: ["catalog","category","item-type","limit","offset","setLimit","setlimit","sysparm-catalog","sysparm-category","sysparm-limit","sysparm-offset","sysparm-text","sysparm-type","text"], shorts: "", sub: {} },
        "item": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "item-variables": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "order": { operands: ["sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
        "add-to-cart": { operands: ["sys_id"], values: ["data","field"], shorts: "DF", sub: {} },
        "cart": { operands: [], values: [], shorts: "", sub: {} },
        "cart-update": { operands: ["cart_item_id"], values: ["data","field"], shorts: "DF", sub: {} },
        "cart-remove": { operands: ["cart_item_id"], values: [], shorts: "", sub: {} },
        "cart-empty": { operands: ["sys_id"], values: [], shorts: "", sub: {} },
        "checkout": { operands: [], values: [], shorts: "", sub: {} },
        "submit-order": { operands: [], values: [], shorts: "", sub: {} },
        "wishlist": { operands: [], values: [], shorts: "", sub: {} },
      },
    },
    "decision": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: [], values: ["display-value","limit","offset","query","setLimit","setlimit","sysparm-display-value","sysparm-limit","sysparm-offset","sysparm-query"], shorts: "q", sub: {} },
        "show": { operands: ["table"], values: [], shorts: "", sub: {} },
        "run": { operands: ["table"], values: ["input"], shorts: "i", sub: {} },
      },
    },
    "playbook": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "list": { operands: ["table","sys_id"], values: [], shorts: "", sub: {} },
        "trigger": { operands: ["table","sys_id"], values: ["scoped-name"], shorts: "", sub: {} },
        "launch": { operands: ["definition"], values: ["input","record"], shorts: "", sub: {} },
      },
    },
    "identify": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "create-update": { operands: [], values: ["data","data-source","field"], shorts: "DF", sub: {} },
        "query": { operands: [], values: ["data","data-source","field"], shorts: "DF", sub: {} },
        "create-update-enhanced": { operands: [], values: ["data","data-source","field","options"], shorts: "DF", sub: {} },
        "query-enhanced": { operands: [], values: ["data","data-source","field","options"], shorts: "DF", sub: {} },
      },
    },
    "user": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "me": { operands: [], values: [], shorts: "", sub: {} },
      },
    },
    "ping": { operands: [], values: [], shorts: "", sub: {} },
    "impersonate": { operands: ["user","command"], values: [], shorts: "", sub: {} },
    "doctor": { operands: [], values: ["need-plugin","need-property","need-role"], shorts: "", sub: {} },
    "open": { operands: ["table","sys_id"], values: ["query","sysparm-query"], shorts: "q", sub: {} },
    "raw": { operands: ["method","path"], values: ["data","field","header","query"], shorts: "DFHq", sub: {} },
    "gr": { operands: ["table"], values: ["display-value","fields","limit","offset","query","setLimit","setlimit","sysparm-display-value","sysparm-limit","sysparm-offset"], shorts: "fq", sub: {} },
    "graphql": { operands: ["query"], values: ["operation","var","variables"], shorts: "", sub: {} },
    "script": {
      operands: [],
      values: [],
      shorts: "",
      sub: {
        "run": { operands: ["script"], values: ["scope"], shorts: "", sub: {} },
      },
    },
    "completion": { operands: ["shell"], values: [], shorts: "", sub: {} },
  },
}
