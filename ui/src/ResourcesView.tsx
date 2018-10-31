import * as React from "react";

import Paper from "@material-ui/core/Paper";
import Popover from "@material-ui/core/Popover";
import Table from "@material-ui/core/Table";
import TableBody from "@material-ui/core/TableBody";
import TableCell from "@material-ui/core/TableCell";
import TableHead from "@material-ui/core/TableHead";
import TableRow from "@material-ui/core/TableRow";

import {
    IDeployerResourceState,
    IResourceStatus,
    IResourceVersion,
    IUiData
} from "./index";

interface IResourceHistoryProps {
    resourceStatus: IResourceStatus;
    statusByEnv: Array<{ env: string; status: IDeployerResourceState }>;
}

class ResourceHistory extends React.Component<IResourceHistoryProps> {
    public state = {
        popoverElem: null,
        popoverText: null
    };

    public handlePopoverOpen = (
        version: IResourceVersion,
        event: React.MouseEvent
    ) => {
        this.setState({
            popoverElem: event.currentTarget,
            popoverText: version.version
        });
    };

    public handlePopoverClose = (event: React.MouseEvent) => {
        if (this.state.popoverElem === event.currentTarget) {
            this.setState({ popoverElem: null });
        }
    };

    public render() {
        const resource = this.props.resourceStatus;
        const versions = Object.keys(resource.versions)
            .map(v => resource.versions[v])
            .reverse();
        let x = 30;
        const versionsAndEnvs = [];
        for (const v of versions) {
            for (const env of Object.keys(resource.version_by_env)) {
                if (resource.version_by_env[env] === v.version_id) {
                    const w = 50;
                    versionsAndEnvs.push(
                        <g>
                            <rect
                                key={env}
                                x={x - 8}
                                y={25 - 8}
                                width={w}
                                height={16}
                                fill="green"
                                stroke="darkGreen"
                                strokeWidth={2}
                                rx={6}
                                ry={6}
                            />
                            <text
                                fill="white"
                                textAnchor="start"
                                alignmentBaseline="middle"
                                x={x - 2}
                                y={25}
                            >
                                {env}
                            </text>
                        </g>
                    );
                    x += w + 9;
                }
            }
            versionsAndEnvs.push(
                <circle
                    key={v.version}
                    cx={x}
                    cy={25}
                    r={8}
                    fill="green"
                    stroke="darkGreen"
                    strokeWidth={2}
                    onMouseEnter={this.handlePopoverOpen.bind(this, v)}
                    onMouseLeave={this.handlePopoverClose}
                />
            );
            x += 25;
        }
        return (
            <div>
                <svg
                    viewBox={`0 0 ${x + 25} 50`}
                    xmlns="http://www.w3.org/2000/svg"
                    style={{ width: x + 25, height: 50 }}
                >
                    {x > 55 && (
                        <line
                            x1={30}
                            y1={25}
                            x2={x - 25}
                            y2={25}
                            stroke="darkGreen"
                            strokeWidth={2}
                        />
                    )}
                    {versionsAndEnvs}
                </svg>
                <Popover
                    style={{ top: 10, pointerEvents: "none" }}
                    open={!!this.state.popoverElem}
                    anchorEl={this.state.popoverElem}
                    anchorOrigin={{
                        vertical: "bottom",
                        horizontal: "center"
                    }}
                    transformOrigin={{
                        vertical: "top",
                        horizontal: "center"
                    }}
                    onClose={this.handlePopoverClose}
                    disableRestoreFocus
                >
                    {this.state.popoverText}
                </Popover>
            </div>
        );
    }
}

interface IResourcesViewProps {
    data: IUiData;
}

export function ResourcesView(props: IResourcesViewProps) {
    const lines = [];
    for (const name of Object.keys(props.data.resources)) {
        const resource = props.data.resources[name];
        const statusByEnv = Object.keys(props.data.deployers).map(env => ({
            env,
            status: props.data.deployers[env].status_by_resource[name]
        }));
        lines.push(
            <TableRow key={resource.name}>
                <TableCell>{resource.name}</TableCell>
                <TableCell>
                    <pre>{JSON.stringify(statusByEnv, null, 4)}</pre>
                </TableCell>
                <TableCell>
                    <pre>{JSON.stringify(resource, null, 4)}</pre>
                </TableCell>
                <TableCell>
                    <ResourceHistory
                        resourceStatus={resource}
                        statusByEnv={statusByEnv}
                    />
                </TableCell>
            </TableRow>
        );
    }
    return (
        <Paper>
            <Table>
                <TableBody>{lines}</TableBody>
            </Table>
        </Paper>
    );
}
