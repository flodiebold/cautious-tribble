import * as React from "react";

import Paper from "@material-ui/core/Paper";
import Table from "@material-ui/core/Table";
import TableBody from "@material-ui/core/TableBody";
import TableCell from "@material-ui/core/TableCell";
import TableHead from "@material-ui/core/TableHead";
import TableRow from "@material-ui/core/TableRow";

import { IUiData } from "./index";

interface IResourcesViewProps {
    data: IUiData;
}

export class ResourcesView extends React.Component<IResourcesViewProps> {
    public render() {
        const lines = [];
        for (const name of Object.keys(this.props.data.resources)) {
            const resource = this.props.data.resources[name];
            lines.push(
                <TableRow>
                    <TableCell>{resource.name}</TableCell>
                    <TableCell>
                        <pre>{JSON.stringify(resource, null, 4)}</pre>
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
