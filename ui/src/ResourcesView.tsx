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
            popoverText: version.change_log
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
        let x = 2;
        const versionsAndEnvs = [];
        const r = 15;
        for (const v of versions) {
            for (const env of Object.keys(resource.version_by_env)) {
                if (resource.version_by_env[env] === v.version_id) {
                    const w = env.length * 8;
                    versionsAndEnvs.push(
                        <g>
                            <rect
                                key={env}
                                x={x}
                                y={2}
                                width={w}
                                height={r * 2}
                                fill="#8C7"
                                stroke="#232"
                                strokeWidth={2}
                                rx={10}
                                ry={10}
                            />
                            <text
                                fontFamily="monospace"
                                stroke="#232"
                                textAnchor="start"
                                alignmentBaseline="middle"
                                x={x + 2}
                                y={r + 2}
                            >
                                {env}
                            </text>
                        </g>
                    );
                    x += w + 4;
                }
            }
            versionsAndEnvs.push(
                <circle
                    key={v.version}
                    cx={x + 8 + 2}
                    cy={r + 2}
                    r={8}
                    fill="#8C7"
                    stroke="#232"
                    strokeWidth={2}
                    onMouseEnter={this.handlePopoverOpen.bind(this, v)}
                    onMouseLeave={this.handlePopoverClose}
                />
            );
            x += 8 * 2 + 4;
        }
        return (
            <div>
                <svg
                    viewBox={`0 0 ${x + r * 2 + 4} ${r * 2 + 4}`}
                    xmlns="http://www.w3.org/2000/svg"
                    style={{ width: x + r * 2 + 4, height: r * 2 + 4 }}
                >
                    {x > 55 && (
                        <line
                            x1={r}
                            y1={r + 2}
                            x2={x - r}
                            y2={r + 2}
                            stroke="black"
                        />
                    )}
                    {versionsAndEnvs}
                </svg>
                <Popover
                    style={{
                        top: 10,
                        pointerEvents: "none",
                        whiteSpace: "pre"
                    }}
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
