import * as React from "react";

import Grid from "@material-ui/core/Grid";
import Paper from "@material-ui/core/Paper";
import Table from "@material-ui/core/Table";
import TableBody from "@material-ui/core/TableBody";
import TableCell from "@material-ui/core/TableCell";
import TableHead from "@material-ui/core/TableHead";
import TableRow from "@material-ui/core/TableRow";

import { IUiData } from "./index";

interface IHistoryViewProps {
    data: IUiData;
}

export class HistoryView extends React.Component<IHistoryViewProps> {
    public render() {
        const history = this.props.data.history;
        return (
            <Grid container spacing={16} style={{ padding: 16 }}>
                <Grid item xs={12}>
                    <Paper>
                        <Table>
                            {history.reverse().map(commit => (
                                <TableRow>
                                    <TableCell>t</TableCell>
                                    <TableCell
                                        style={{ whiteSpace: "pre-line" }}
                                    >
                                        {commit.message}
                                    </TableCell>
                                    <TableCell>
                                        <pre>
                                            {JSON.stringify(commit, null, 4)}
                                        </pre>
                                    </TableCell>
                                </TableRow>
                            ))}
                        </Table>
                    </Paper>
                </Grid>
            </Grid>
        );
    }
}
