import * as React from "react";
import * as ReactDOM from "react-dom";

import AppBar from "@material-ui/core/AppBar";
import Tab from "@material-ui/core/Tab";
import Tabs from "@material-ui/core/Tabs";

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
}

interface IDeployerStatusMessage {
    type: "IDeployerStatus";
    counter: number;
    deployers: { [key: string]: IDeployerStatus };
}

interface ITransitionStatusMessage {
    type: "ITransitionStatus";
    counter: number;
    transitions: { [key: string]: ITransitionStatus };
}

type Message =
    | IFullStatusMessage
    | IDeployerStatusMessage
    | ITransitionStatusMessage;

interface IUiData {
    counter: number;
    deployers: { [key: string]: IDeployerStatus };
    transitions: { [key: string]: ITransitionStatus };
}

class Page extends React.Component<{}, { tab: number; data: IUiData }> {
    constructor(props: {}) {
        super(props);
        this.state = {
            data: {
                counter: 0,
                deployers: {},
                transitions: {}
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
                message.type === "IDeployerStatus"
            ) {
                Object.assign(data.deployers, message.deployers);
            }

            if (
                message.type === "FullStatus" ||
                message.type === "ITransitionStatus"
            ) {
                Object.assign(data.transitions, message.transitions);
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
                        <Tab label="Stuff" />
                        <Tab label="Data" />
                    </Tabs>
                </AppBar>
                {this.state.tab === 1 && (
                    <pre>{JSON.stringify(this.state.data, null, 4)}</pre>
                )}
            </div>
        );
    }
}

ReactDOM.render(<Page />, document.getElementById("main"));
