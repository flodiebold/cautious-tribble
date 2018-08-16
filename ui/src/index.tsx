import * as React from "react";
import * as ReactDOM from "react-dom";

const ws = new WebSocket("ws://" + document.location.host + "/api");

ws.onopen = ev => {};

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

const uiData: UiData = {
    counter: 0,
    deployers: {},
    transitions: {}
};

ws.onmessage = ev => {
    const data: Message = JSON.parse(ev.data);

    if (data.type === "FullStatus" || data.type === "DeployerStatus") {
        Object.assign(uiData.deployers, data.deployers);
    }

    if (data.type === "FullStatus" || data.type === "TransitionStatus") {
        Object.assign(uiData.transitions, data.transitions);
    }

    uiData.counter = data.counter;

    ReactDOM.render(
        <pre>{JSON.stringify(uiData, null, 4)}</pre>,
        document.getElementById("main")
    );
};

ReactDOM.render(<div />, document.getElementById("main"));
