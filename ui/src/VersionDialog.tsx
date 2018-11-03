import * as React from "react";
// @ts-ignore
import { useState } from "react";

import Button from "@material-ui/core/Button";
import Checkbox from "@material-ui/core/Checkbox";
import Dialog from "@material-ui/core/Dialog";
import DialogActions from "@material-ui/core/DialogActions";
import DialogContent from "@material-ui/core/DialogContent";
import DialogContentText from "@material-ui/core/DialogContentText";
import DialogTitle from "@material-ui/core/DialogTitle";
import Divider from "@material-ui/core/Divider";
import FormControl from "@material-ui/core/FormControl";
import FormControlLabel from "@material-ui/core/FormControlLabel";
import FormGroup from "@material-ui/core/FormGroup";
import FormHelperText from "@material-ui/core/FormHelperText";
import FormLabel from "@material-ui/core/FormLabel";
import TextField from "@material-ui/core/TextField";
import withMobileDialog from "@material-ui/core/withMobileDialog";

import { IResourceVersion } from ".";

export interface IVersionDialogProps {
    onClose: () => void;
    resource: string;
    deployableEnvs: string[];
    version: IResourceVersion;
}

export function VersionDialog(props: IVersionDialogProps) {
    const { onClose, version, resource, deployableEnvs } = props;
    const [deployEnvs, setDeployEnvs] = useState({});
    const [reasonMessage, setReasonMessage] = useState("");
    const deployEnabled =
        Object.keys(deployEnvs).some(k => !!deployEnvs[k]) &&
        reasonMessage !== "";
    return (
        <div>
            <Dialog
                open
                onClose={onClose}
                aria-labelledby="responsive-dialog-title"
            >
                <DialogTitle id="responsive-dialog-title">
                    {resource} {version.version}
                </DialogTitle>
                <DialogContent>
                    <DialogContentText style={{ whiteSpace: "pre" }}>
                        {version.change_log}
                    </DialogContentText>
                </DialogContent>
                <Divider />
                <DialogContent>
                    <FormControl component="fieldset" margin="normal">
                        <FormLabel component="legend">Deploy to</FormLabel>
                        <FormGroup>
                            {deployableEnvs.map(env => (
                                <FormControlLabel
                                    control={
                                        <Checkbox
                                            checked={!!deployEnvs[env]}
                                            onChange={e =>
                                                setDeployEnvs(
                                                    Object.assign(
                                                        {},
                                                        deployEnvs,
                                                        {
                                                            [env]:
                                                                e.target.checked
                                                        }
                                                    )
                                                )
                                            }
                                            value={env}
                                        />
                                    }
                                    label={env}
                                />
                            ))}
                        </FormGroup>
                        <TextField
                            id="reason-message"
                            label="Reason message"
                            multiline
                            rows="4"
                            value={reasonMessage}
                            onChange={e => setReasonMessage(e.target.value)}
                            margin="none"
                            variant="filled"
                        />
                    </FormControl>
                </DialogContent>
                <DialogActions disableActionSpacing>
                    <Button onClick={onClose} color="primary" autoFocus>
                        Cancel
                    </Button>
                    <Button
                        onClick={onClose}
                        color="primary"
                        disabled={!deployEnabled}
                    >
                        Deploy
                    </Button>
                </DialogActions>
            </Dialog>
        </div>
    );
}
