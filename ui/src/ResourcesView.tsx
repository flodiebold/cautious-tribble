import * as React from "react";

import Paper from "@material-ui/core/Paper";
import Popover from "@material-ui/core/Popover";
import Table from "@material-ui/core/Table";
import TableBody from "@material-ui/core/TableBody";
import TableCell from "@material-ui/core/TableCell";
import TableHead from "@material-ui/core/TableHead";
import TableRow from "@material-ui/core/TableRow";

import { IDeployerResourceState, IResourceVersion, IUiData } from "./index";

interface IResourceHistoryProps {
    versions: IResourceVersion[];
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
        const reversed = this.props.versions.slice().reverse();
        return (
            <div>
                <svg
                    viewBox="0 0 200 50"
                    xmlns="http://www.w3.org/2000/svg"
                    style={{ width: 200, height: 50 }}
                >
                    {reversed.length > 0 && (
                        <line
                            x1={30}
                            y1={25}
                            x2={5 + reversed.length * 25}
                            y2={25}
                            stroke="darkGreen"
                            strokeWidth={2}
                        />
                    )}
                    {reversed.map((v, i) => (
                        <circle
                            key={v.version}
                            cx={30 + i * 25}
                            cy={25}
                            r={8}
                            fill="green"
                            stroke="darkGreen"
                            strokeWidth={2}
                            onMouseEnter={this.handlePopoverOpen.bind(this, v)}
                            onMouseLeave={this.handlePopoverClose}
                        />
                    ))}
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

export class ResourcesView extends React.Component<IResourcesViewProps> {
    public render() {
        const lines = [];
        for (const name of Object.keys(this.props.data.resources)) {
            const resource = this.props.data.resources[name];
            const statusByEnv = Object.keys(this.props.data.deployers).map(
                env => ({
                    env,
                    status: this.props.data.deployers[env].status_by_resource[
                        name
                    ]
                })
            );
            const versions = Object.keys(resource.versions).map(
                v => resource.versions[v]
            );
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
                            versions={versions}
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
}
