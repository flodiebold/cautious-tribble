import * as React from "react";

import Paper from "@material-ui/core/Paper";
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
    public render() {
        return (
            <svg
                viewBox="0 0 200 50"
                xmlns="http://www.w3.org/2000/svg"
                style={{ width: 200, height: 50 }}
            >
                {this.props.versions.map((v, i) => (
                    <circle
                        cx={30 + i * 25}
                        cy={25}
                        r={10}
                        fill="green"
                        stroke="darkGreen"
                        strokeWidth={3}
                    />
                ))}
            </svg>
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
                <TableRow>
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
