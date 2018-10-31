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

export function HistoryView(props: IHistoryViewProps) {
    const history = props.data.history;
    const reversed = props.data.history.slice().reverse();
    return (
        <Grid container spacing={16} style={{ padding: 16 }}>
            <Grid item xs={12}>
                <Paper>
                    <Table>
                        <TableBody>
                            {reversed.map(commit => (
                                <TableRow key={commit.id}>
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
                        </TableBody>
                    </Table>
                </Paper>
            </Grid>
        </Grid>
    );
}
