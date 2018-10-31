import * as React from "react";
import * as ReactDOM from "react-dom";

import AppBar from "@material-ui/core/AppBar";
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
}

export interface IResourceStatus {
    name: string;
    versions: { [id: string]: IResourceVersion };
    base_data: { [env: string]: string };
    version_by_env: { [env: string]: string };
}

export type IChangeVersion = {
    change: "Version";
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

class Page extends React.Component<{}, { tab: number; data: IUiData }> {
    constructor(props: {}) {
        super(props);
        this.state = {
            data: {
                counter: 0,
                deployers: {},
                transitions: {},
                resources: {},
                history: []
            },
            tab: 0
        };

        const host = document.location ? document.location.host : "";

        const ws = new WebSocket("ws://" + host + "/api");

        ws.onmessage = this.handleWebSocketMessage;
    }

    public handleWebSocketMessage = (ev: MessageEvent) => {
        const message: Message = JSON.parse(ev.data);

        this.setState(state => {
            const data = state.data;
            if (
                message.type === "FullStatus" ||
                message.type === "DeployerStatus"
            ) {
                Object.assign(data.deployers, message.deployers);
            }

            if (
                message.type === "FullStatus" ||
                message.type === "TransitionStatus"
            ) {
                Object.assign(data.transitions, message.transitions);
            }

            if (message.type === "FullStatus" || message.type === "Versions") {
                Object.assign(data.resources, message.resources);
                data.history = message.history;
            }

            data.counter = message.counter;
            return { data };
        });
    };

    public handleTabChange = (ev: any, tab: number) => {
        this.setState({ tab });
    };

    public render() {
        return (
            <div>
                <AppBar position="static">
                    <Tabs
                        value={this.state.tab}
                        onChange={this.handleTabChange}
                    >
                        <Tab label="Resources" />
                        <Tab label="History" />
                        <Tab label="Data" />
                    </Tabs>
                </AppBar>
                {this.state.tab === 0 && (
                    <ResourcesView data={this.state.data} />
                )}
                {this.state.tab === 1 && <HistoryView data={this.state.data} />}
                {this.state.tab === 2 && (
                    <pre>{JSON.stringify(this.state.data, null, 4)}</pre>
                )}
            </div>
        );
    }
}

ReactDOM.render(<Page />, document.getElementById("main"));
