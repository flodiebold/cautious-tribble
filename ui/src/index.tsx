import * as React from "react";
import * as ReactDOM from "react-dom";

import AppBar from "@material-ui/core/AppBar";
import Tab from "@material-ui/core/Tab";
import Tabs from "@material-ui/core/Tabs";

import { ResourcesView } from "./ResourcesView";

interface IDeployerStatus {
    deployed_version: string;
    last_successfully_deployed_version: string | null;
    rollout_status: "InProgress" | "Clean" | "Outdated" | "Failed";
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

interface IResourceVersion {
    version_id: string;
    introduced_in: string;
    version: string;
}

interface IResourceStatus {
    name: string;
    versions: { [id: string]: IResourceVersion };
    base_data: { [env: string]: string };
    version_by_env: { [env: string]: string };
}

type IChangeVersion = {
    change: "Version";
} & IResourceVersion;

interface IChangeDeployable {
    change: "Deployable";
    resource: string;
    env: string;
    content_id: string;
}

interface IChangeBaseData {
    change: "BaseData";
    resource: string;
    env: string;
    content_id: string;
}

interface IVersionDeployed {
    change: "VersionDeployed";
    resource: string;
    env: string;
    version_id: string;
}

type ResourceRepoChange =
    | IChangeVersion
    | IChangeDeployable
    | IChangeBaseData
    | IVersionDeployed;

interface IResourceRepoCommit {
    id: string;
    message: string;
    changes: ResourceRepoChange[];
}

interface IVersionsMessage {
    type: "Versions";
    counter: number;
    resources: { [name: string]: IResourceStatus };
    history: IResourceRepoCommit[];
}

type Message =
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

        const ws = new WebSocket("ws://" + document.location.host + "/api");

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
                        <Tab label="Data" />
                    </Tabs>
                </AppBar>
                {this.state.tab === 0 && (
                    <ResourcesView data={this.state.data} />
                )}
                {this.state.tab === 1 && (
                    <pre>{JSON.stringify(this.state.data, null, 4)}</pre>
                )}
            </div>
        );
    }
}

ReactDOM.render(<Page />, document.getElementById("main"));
