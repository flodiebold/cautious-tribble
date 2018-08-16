import * as React from "react";
import * as ReactDOM from "react-dom";
import AppBar from "@material-ui/core/AppBar";
import Tabs from "@material-ui/core/Tabs";
import Tab from "@material-ui/core/Tab";

interface DeployerStatus {
    deployed_version: string;
    last_successfully_deployed_version: string | null;
    rollout_status: "InProgress" | "Clean" | "Outdated" | "Failed";
}

interface TransitionStatus {
    successful_runs: Array<{ time: string; committed_version: string }>;
    last_run: null | {
        time: string | null;
        result: "Success" | "Skipped" | "Blocked" | "CheckFailed";
    };
}

interface FullStatusMessage {
    type: "FullStatus";
    counter: number;
    deployers: { [key: string]: DeployerStatus };
    transitions: { [key: string]: TransitionStatus };
}

interface DeployerStatusMessage {
    type: "DeployerStatus";
    counter: number;
    deployers: { [key: string]: DeployerStatus };
}

interface TransitionStatusMessage {
    type: "TransitionStatus";
    counter: number;
    transitions: { [key: string]: TransitionStatus };
}

type Message =
    | FullStatusMessage
    | DeployerStatusMessage
    | TransitionStatusMessage;

interface UiData {
    counter: number;
    deployers: { [key: string]: DeployerStatus };
    transitions: { [key: string]: TransitionStatus };
}

class Page extends React.Component<{}, { tab: number; data: UiData }> {
    constructor(props: {}) {
        super(props);
        this.state = {
            tab: 0,
            data: {
                counter: 0,
                deployers: {},
                transitions: {}
            }
        };

        const ws = new WebSocket("ws://" + document.location.host + "/api");

        ws.onopen = ev => {};
        ws.onmessage = this.handleWebSocketMessage;
    }

    handleWebSocketMessage = (ev: MessageEvent) => {
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

            data.counter = message.counter;
            return { data };
        });
    };

    handleTabChange = (ev: any, tab: number) => {
        this.setState({ tab });
    };

    render() {
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
