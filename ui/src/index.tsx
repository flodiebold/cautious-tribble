import * as React from "react";
import { useEffect, useReducer, useRef, useState } from "react";
import * as ReactDOM from "react-dom";

import AppBar from "@material-ui/core/AppBar";
import { createMuiTheme, MuiThemeProvider } from "@material-ui/core/styles";
import Tab from "@material-ui/core/Tab";
import Tabs from "@material-ui/core/Tabs";

import { HistoryView } from "./HistoryView";
import { ResourcesView } from "./ResourcesView";

export type IDeployerResourceState =
    | { state: "NotDeployed" }
    | {
          state: "Deployed";
          version: string;
          expected_version: string;
          reason:
              | "Clean"
              | "Failed"
              | "NotYetObserved"
              | "NotAllUpdated"
              | "OldReplicasPending"
              | "UpdatedUnavailable"
              | "NoStatus";
          message?: string;
          expected?: number;
          updated?: number;
          number?: number;
          available?: number;
      };

interface IDeployerStatus {
    deployed_version: string;
    last_successfully_deployed_version: string | null;
    rollout_status: "InProgress" | "Clean" | "Outdated" | "Failed";
    status_by_resource: { [resource: string]: IDeployerResourceState };
}

interface ITransitionStatus {
    successful_runs: Array<{ time: string; committed_version: string }>;
    last_run: null | {
        time: string | null;
        result: "Success" | "Skipped" | "Blocked" | "CheckFailed";
    };
}

interface IFullStatusMessage {
    type: "FullStatus";
    counter: number;
    deployers: { [key: string]: IDeployerStatus };
    transitions: { [key: string]: ITransitionStatus };
    resources: any;
    history: any;
}

interface IDeployerStatusMessage {
    type: "DeployerStatus";
    counter: number;
    deployers: { [key: string]: IDeployerStatus };
}

interface ITransitionStatusMessage {
    type: "TransitionStatus";
    counter: number;
    transitions: { [key: string]: ITransitionStatus };
}

export interface IResourceVersion {
    version_id: string;
    introduced_in: string;
    version: string;
    change_log: string;
}

export interface IResourceStatus {
    name: string;
    versions: { [id: string]: IResourceVersion };
    base_data: { [env: string]: string };
    version_by_env: { [env: string]: string };
}

export type IChangeVersion = {
    change: "Version";
    resource: string;
} & IResourceVersion;

export interface IChangeDeployable {
    change: "Deployable";
    resource: string;
    env: string;
    content_id: string;
}

export interface IChangeBaseData {
    change: "BaseData";
    resource: string;
    env: string;
    content_id: string;
}

export interface IVersionDeployed {
    change: "VersionDeployed";
    resource: string;
    env: string;
    previous_version_id: string | null;
    version_id: string;
}

export type ResourceRepoChange =
    | IChangeVersion
    | IChangeDeployable
    | IChangeBaseData
    | IVersionDeployed;

export interface IResourceRepoCommit {
    id: string;
    message: string;
    long_message: string;
    author_name: string;
    author_email: string;
    time: string;
    changes: ResourceRepoChange[];
}

export interface IVersionsMessage {
    type: "Versions";
    counter: number;
    resources: { [name: string]: IResourceStatus };
    history: IResourceRepoCommit[];
}

export type Message =
    | IFullStatusMessage
    | IDeployerStatusMessage
    | ITransitionStatusMessage
    | IVersionsMessage;

export interface IUiData {
    counter: number;
    deployers: { [key: string]: IDeployerStatus };
    transitions: { [key: string]: ITransitionStatus };
    resources: { [name: string]: IResourceStatus };
    history: IResourceRepoCommit[];
}

function useWebSocket(url: string, onMessage: (ev: MessageEvent) => void) {
    const ws: { current: WebSocket | null } = useRef(null);
    const timeout: { current: number | null } = useRef(null);
    const connect = () => {
        ws.current = new WebSocket(url);
        ws.current.onmessage = onMessage;
        ws.current.onclose = (ev: CloseEvent) => {
            if (ev.code === 1000) {
                // ok
                return;
            }
            timeout.current = setTimeout(
                connect,
                4000 /* TODO make this configurable */
            );
        };
    };
    useEffect(() => {
        connect();
        return () => {
            if (ws.current !== null) {
                ws.current.close(1000, "going away");
                ws.current = null;
            }
            if (timeout.current !== null) {
                clearTimeout(timeout.current);
                timeout.current = null;
            }
        };
    }, [url]);
}

function applyMessage(data: IUiData, message: Message): IUiData {
    if (message.type === "FullStatus" || message.type === "DeployerStatus") {
        Object.assign(data.deployers, message.deployers);
    }

    if (message.type === "FullStatus" || message.type === "TransitionStatus") {
        Object.assign(data.transitions, message.transitions);
    }

    if (message.type === "FullStatus" || message.type === "Versions") {
        Object.assign(data.resources, message.resources);
        data.history = message.history;
    }

    data.counter = message.counter;
    return data;
}

const theme = createMuiTheme({
    typography: {
        useNextVariants: true
    }
});

function Page() {
    const [tab, setTab] = useState(0);
    const [data, dispatchMessage]: [IUiData, (m: Message) => void] = useReducer(
        applyMessage,
        {
            counter: 0,
            deployers: {},
            transitions: {},
            resources: {},
            history: []
        }
    );
    const host = document.location ? document.location.host : "";

    useWebSocket("ws://" + host + "/api", (ev: MessageEvent) => {
        const message: Message = JSON.parse(ev.data);
        dispatchMessage(message);
    });

    return (
        <MuiThemeProvider theme={theme}>
            <div>
                <AppBar position="static">
                    <Tabs
                        value={tab}
                        onChange={(ev: any, newTab: number) => setTab(newTab)}>
                        <Tab label="Resources" />
                        <Tab label="History" />
                        <Tab label="Data" />
                    </Tabs>
                </AppBar>
                {tab === 0 && <ResourcesView data={data} />}
                {tab === 1 && <HistoryView data={data} />}
                {tab === 2 && <pre>{JSON.stringify(data, null, 4)}</pre>}
            </div>
        </MuiThemeProvider>
    );
}

ReactDOM.render(<Page />, document.getElementById("main"));
// ReactDOM.createRoot(document.getElementById("main")).render(<Page />);
